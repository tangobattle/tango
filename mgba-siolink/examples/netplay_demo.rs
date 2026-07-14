//! A minimal multiplayer emulator frontend over mgba-siolink, speaking
//! tango's real transport stack: two instances load the same GBA ROM,
//! link up over WebRTC ([`tango_rtc`], either signaling-free direct or
//! matched through a [`tango_signaling`] server), and exchange per-tick
//! inputs as [`rennet`] frames on an unreliable data channel — loss is
//! healed by rennet's redundancy window, latency by rollback.
//!
//!   # direct, no server (host listens on a UDP port):
//!   cargo run --release --example netplay_demo -- \
//!       --host 35835 game.gba --save p1.sav
//!   cargo run --release --example netplay_demo -- \
//!       --connect 127.0.0.1:35835 game.gba --save p2.sav
//!
//!   # or matched by link code through a signaling server:
//!   cargo run --release --example netplay_demo -- \
//!       --session some-code game.gba
//!
//! Controls: arrows = D-pad, X = A, Z = B, A = L, S = R,
//! Return = Start, Backspace = Select, Escape = quit.
//!
//! Flags: --save FILE (this side's SRAM), --remote-rom FILE (your copy
//! of the opponent's game when it differs from yours — crossplay, e.g.
//! Gregar vs Falzar; ROM bytes never cross the wire, each side must own
//! both games), --delay N (present delay: how many ticks behind the
//! frontier to display — purely local, the peers need not agree),
//! --signaling URL (default: tango's public matchmaking server),
//! --show-remote (render the opponent's screen too), --mute,
//! --headless (no window/audio), --frames N (auto-exit), --wiggle
//! (deterministic key mashing, for exercising rollback), --rom testrom
//! (built-in SIO ping-pong ROM instead of a file).
//!
//! Two data channels, mirroring tango's split: a reliable ordered control
//! channel for the hello (ROM checksum, match clock, both sides' save
//! images) plus periodic state checksums, and an unreliable unordered
//! channel where every frame carries the rennet redundancy window of
//! recent inputs plus the sender's tick advantage. The advantages feed
//! the tango throttler on each side: the leading peer shaves its tick
//! rate until the clocks realign. A checksum mismatch means desync; the
//! demo says so loudly and exits.

use mgba_siolink::session::Session;
use mgba_siolink::throttler::Throttler;
use mgba_siolink::{testrom, Pair, PairOptions, SideOptions};

const GBA_FPS: f64 = 59.7275005696;
const CHECKPOINT_EVERY: u32 = 60;
/// rennet stream horizon: how much unacked/undelivered input either side
/// tolerates. This is also the speculation bound — OutStream::push past
/// the horizon silently sheds elements the peer never got (= a broken
/// stream), so the stall guard below must engage first.
const HORIZON: u32 = 120;
/// Stop advancing when the unconfirmed/unacked backlog gets this close
/// to the horizon; the peer is too far behind (or gone) and we'd rather
/// stall than desync fatally.
const STALL_MARGIN: u32 = 12;
/// Distinct from tango's protocol version so a demo instance can never be
/// matched with a real tango client by the signaling server — and bumped
/// from \x01: the hello lost the negotiated input delay and data frames
/// grew the tick-advantage meta.
const DEMO_PROTOCOL_VERSION: u32 = 0x53494f02; // "SIO\x02"
const DEFAULT_SIGNALING: &str = "wss://matchmaking.tango.n1gp.net";

struct Args {
    host_port: Option<u16>,
    connect: Option<String>,
    session: Option<String>,
    signaling: String,
    rom: Option<String>,
    remote_rom: Option<String>,
    save: Option<String>,
    delay: u32,
    frames: Option<u32>,
    headless: bool,
    mute: bool,
    show_remote: bool,
    /// Mash a deterministic key pattern on top of the keyboard, so
    /// predictions actually miss and rollbacks fire.
    wiggle: bool,
}

fn parse_args() -> Args {
    let mut args = Args {
        host_port: None,
        connect: None,
        session: None,
        signaling: DEFAULT_SIGNALING.to_owned(),
        rom: None,
        remote_rom: None,
        save: None,
        delay: 2,
        frames: None,
        headless: false,
        mute: false,
        show_remote: false,
        wiggle: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let mut val = || it.next().unwrap_or_else(|| die(&format!("{a} needs a value")));
        match a.as_str() {
            "--host" => args.host_port = Some(val().parse().unwrap_or_else(|_| die("bad --host port"))),
            "--connect" => args.connect = Some(val()),
            "--session" => args.session = Some(val()),
            "--signaling" => args.signaling = val(),
            "--remote-rom" => args.remote_rom = Some(val()),
            "--save" => args.save = Some(val()),
            "--delay" => args.delay = val().parse().unwrap_or_else(|_| die("bad --delay")),
            "--frames" => args.frames = Some(val().parse().unwrap_or_else(|_| die("bad --frames"))),
            "--rom" => args.rom = Some(val()),
            "--headless" => args.headless = true,
            "--wiggle" => args.wiggle = true,
            "--mute" => args.mute = true,
            "--show-remote" => args.show_remote = true,
            _ if !a.starts_with('-') && args.rom.is_none() => args.rom = Some(a),
            _ => die(&format!("unknown argument {a}")),
        }
    }
    let transports = [args.host_port.is_some(), args.connect.is_some(), args.session.is_some()];
    if transports.iter().filter(|t| **t).count() != 1 {
        die("pass exactly one of --host PORT, --connect ADDR:PORT, or --session CODE");
    }
    if args.rom.is_none() {
        die("pass a ROM path (or --rom testrom)");
    }
    args
}

fn die(msg: &str) -> ! {
    eprintln!("netplay_demo: {msg}");
    std::process::exit(1);
}

// ---- transport -----------------------------------------------------------

/// The demo's two data channels, mirroring tango's control/in-match split.
/// Own labels so a demo instance and a real tango client can't half-pair.
fn channel_specs() -> Vec<tango_rtc::ChannelConfig> {
    vec![
        tango_rtc::ChannelConfig {
            label: "siolink".to_owned(),
            ordered: true,
            reliable: true,
        },
        tango_rtc::ChannelConfig {
            label: "siolink-match".to_owned(),
            ordered: false,
            reliable: false,
        },
    ]
}

enum Incoming {
    Control(Vec<u8>),
    Data(Vec<u8>),
    Gone(&'static str),
}

/// The live transport: a WebRTC peer connection with its two channels
/// pumped by background tasks — writes go through unbounded senders,
/// reads arrive merged on one sync receiver.
struct Net {
    /// Keeps the tokio workers driving the connection alive.
    _rt: tokio::runtime::Runtime,
    /// Keeps ICE/DTLS/SCTP alive for the channels' lifetime.
    _peer_conn: tango_rtc::PeerConnection,
    control_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    data_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    incoming: std::sync::mpsc::Receiver<Incoming>,
    /// True when this side is player 0 (the master GBA).
    is_player_0: bool,
}

fn connect_transport(args: &Args) -> Net {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap_or_else(|e| die(&format!("tokio runtime: {e}")));

    let (peer_conn, dcs, events, is_player_0) = if let Some(code) = &args.session {
        // Matchmaking: rendezvous by link code through the signaling server,
        // which relays the SDP offer/answer and ICE candidates.
        eprintln!("connecting to {} with code {code:?}...", args.signaling);
        let connected = rt
            .block_on(async {
                tango_signaling::connect(
                    &args.signaling,
                    code,
                    None, // let ICE pick direct vs relay
                    DEMO_PROTOCOL_VERSION,
                    channel_specs(),
                    None, // no client identity certificate
                )
                .await?
                .await
            })
            .unwrap_or_else(|e| die(&format!("signaling: {e}")));
        // Neither side is structurally "the host", so break the symmetry
        // off the DTLS certificate fingerprints both sides can see.
        let is_player_0 = match connected.local_dtls_fingerprint.cmp(&connected.peer_dtls_fingerprint) {
            std::cmp::Ordering::Less => true,
            std::cmp::Ordering::Greater => false,
            std::cmp::Ordering::Equal => die("cannot assign sides: DTLS fingerprints unavailable"),
        };
        (connected.peer_conn, connected.channels, None, is_player_0)
    } else {
        // Signaling-free direct link: everything an SDP exchange would carry
        // is a fixed constant, the dialer just needs the host's address.
        let role = if let Some(port) = args.host_port {
            eprintln!("hosting on UDP port {port}...");
            tango_rtc::DirectRole::Host { port }
        } else {
            let addr = args.connect.as_ref().unwrap();
            eprintln!("dialing {addr}...");
            let remote = rt
                .block_on(tokio::net::lookup_host(addr.clone()))
                .ok()
                .and_then(|mut it| it.next())
                .unwrap_or_else(|| die(&format!("could not resolve {addr}")));
            tango_rtc::DirectRole::Connect { remote }
        };
        let (peer_conn, dcs, events) = {
            let _guard = rt.enter();
            tango_rtc::PeerConnection::new_direct(tango_rtc::RtcConfig::default(), &channel_specs(), role)
                .unwrap_or_else(|e| die(&format!("direct rtc: {e}")))
        };
        (peer_conn, dcs, Some(events), args.host_port.is_some())
    };

    if let Some(mut events) = events {
        // Drain (and log) connection state changes so a dropped link's cause
        // is visible; also keeps the event sender from backing up.
        rt.spawn(async move {
            while let Some(ev) = events.recv().await {
                if let tango_rtc::PeerConnectionEvent::ConnectionStateChange(state) = ev {
                    eprintln!("peer connection: {state:?}");
                }
            }
        });
    }

    let [control_dc, data_dc] = <[_; 2]>::try_from(dcs)
        .unwrap_or_else(|dcs: Vec<_>| die(&format!("expected 2 data channels, got {}", dcs.len())));
    let (mut control_send, mut control_recv) = control_dc.split();
    let (mut data_send, mut data_recv) = data_dc.split();

    let (control_tx, mut control_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    let (data_tx, mut data_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    let (in_tx, incoming) = std::sync::mpsc::channel();

    rt.spawn(async move {
        while let Some(msg) = control_rx.recv().await {
            if control_send.send(&msg).await.is_err() {
                break;
            }
        }
    });
    rt.spawn(async move {
        while let Some(msg) = data_rx.recv().await {
            if data_send.send(&msg).await.is_err() {
                break;
            }
        }
    });
    let in_tx2 = in_tx.clone();
    rt.spawn(async move {
        loop {
            match control_recv.receive().await {
                Some(msg) => {
                    if in_tx2.send(Incoming::Control(msg)).is_err() {
                        return;
                    }
                }
                None => {
                    let _ = in_tx2.send(Incoming::Gone("control channel closed"));
                    return;
                }
            }
        }
    });
    rt.spawn(async move {
        loop {
            match data_recv.receive().await {
                Some(msg) => {
                    if in_tx.send(Incoming::Data(msg)).is_err() {
                        return;
                    }
                }
                None => {
                    let _ = in_tx.send(Incoming::Gone("data channel closed"));
                    return;
                }
            }
        }
    });

    Net {
        _rt: rt,
        _peer_conn: peer_conn,
        control_tx,
        data_tx,
        incoming,
        is_player_0,
    }
}

// ---- control-channel protocol ---------------------------------------------

const MSG_HELLO: u8 = 0x10;
const MSG_SAVE_CHUNK: u8 = 0x11;
const MSG_CHECKPOINT: u8 = 0x02;
const MSG_BYE: u8 = 0x03;
/// SCTP messages have implementation-dependent size ceilings; save images
/// (32-128KiB) go over in comfortable pieces.
const SAVE_CHUNK: usize = 16 * 1024;

struct Hello {
    rom_crc: u32,
    rtc: u64,
    save: Option<Vec<u8>>,
}

fn send_hello(net: &Net, rom_crc: u32, rtc: u64, save: &Option<Vec<u8>>) {
    let mut msg = vec![MSG_HELLO];
    msg.extend_from_slice(&rom_crc.to_le_bytes());
    msg.extend_from_slice(&rtc.to_le_bytes());
    let save_len = save.as_ref().map_or(u32::MAX, |s| s.len() as u32);
    msg.extend_from_slice(&save_len.to_le_bytes());
    let _ = net.control_tx.send(msg);
    if let Some(save) = save {
        for chunk in save.chunks(SAVE_CHUNK) {
            let mut msg = Vec::with_capacity(1 + chunk.len());
            msg.push(MSG_SAVE_CHUNK);
            msg.extend_from_slice(chunk);
            let _ = net.control_tx.send(msg);
        }
    }
}

/// Receive the peer's hello (+ save chunks) off the ordered control
/// channel. Data-channel frames that race ahead of pair construction are
/// buffered and replayed into the session later.
fn recv_hello(net: &Net, pending_data: &mut Vec<Vec<u8>>) -> Hello {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let mut hello: Option<Hello> = None;
    let mut save_remaining = 0usize;
    loop {
        let timeout = deadline
            .checked_duration_since(std::time::Instant::now())
            .unwrap_or_else(|| die("timed out waiting for the peer's hello"));
        match net.incoming.recv_timeout(timeout) {
            Ok(Incoming::Control(msg)) if msg.first() == Some(&MSG_HELLO) => {
                // Field layout: type(1) crc(4) rtc(8) save_len(4).
                if msg.len() < 17 {
                    die("malformed hello");
                }
                let h = Hello {
                    rom_crc: u32::from_le_bytes(msg[1..5].try_into().unwrap()),
                    rtc: u64::from_le_bytes(msg[5..13].try_into().unwrap()),
                    save: {
                        let save_len = u32::from_le_bytes(msg[13..17].try_into().unwrap());
                        save_remaining = if save_len == u32::MAX { 0 } else { save_len as usize };
                        (save_len != u32::MAX).then(|| Vec::with_capacity(save_remaining))
                    },
                };
                hello = Some(h);
                if save_remaining == 0 {
                    return hello.unwrap();
                }
            }
            Ok(Incoming::Control(msg)) if msg.first() == Some(&MSG_SAVE_CHUNK) => {
                let Some(h) = hello.as_mut() else {
                    die("save chunk before hello");
                };
                let Some(save) = h.save.as_mut() else {
                    die("unexpected save chunk");
                };
                save.extend_from_slice(&msg[1..]);
                save_remaining = save_remaining.saturating_sub(msg.len() - 1);
                if save_remaining == 0 {
                    return hello.take().unwrap();
                }
            }
            Ok(Incoming::Control(_)) => die("unexpected control message during handshake"),
            Ok(Incoming::Data(frame)) => pending_data.push(frame),
            Ok(Incoming::Gone(why)) => die(&format!("peer connection lost during handshake: {why}")),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => die("timed out waiting for the peer's hello"),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => die("transport tasks died"),
        }
    }
}

// ---- rennet input plane ----------------------------------------------------

/// One tick's joypad state, the element at each rennet seq slot
/// (seq = local tick).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Keys(u16);

impl rennet::Codec for Keys {
    fn encode<W: std::io::Write>(&self, w: &mut W) -> std::io::Result<()> {
        w.write_all(&self.0.to_le_bytes())
    }

    fn decode<R: std::io::Read>(r: &mut R) -> std::io::Result<Option<Self>> {
        let mut b = [0u8; 2];
        match r.read_exact(&mut b) {
            Ok(()) => Ok(Some(Keys(u16::from_le_bytes(b)))),
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// Per-frame meta: the sender's tick advantage when its newest input was
/// pushed — their half of the clock-sync handshake the throttler consumes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Adv(i16);

impl rennet::Codec for Adv {
    fn encode<W: std::io::Write>(&self, w: &mut W) -> std::io::Result<()> {
        w.write_all(&self.0.to_le_bytes())
    }

    fn decode<R: std::io::Read>(r: &mut R) -> std::io::Result<Option<Self>> {
        let mut b = [0u8; 2];
        match r.read_exact(&mut b) {
            Ok(()) => Ok(Some(Adv(i16::from_le_bytes(b)))),
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DemoProto;

impl rennet::Protocol for DemoProto {
    type Element = Keys;
    type Meta = Adv;
    const MAX_RUN: usize = HORIZON as usize;
}

// ---- frontend ------------------------------------------------------------

const GBA_W: u32 = 240;
const GBA_H: u32 = 160;
const SCALE: u32 = 3;

const AUDIO_OUT_RATE: u32 = 48000;
/// ~200ms of queued output; rollback re-simulation bursts past this get
/// dropped rather than heard twice (or queued into the future).
const AUDIO_MAX_QUEUED_BYTES: i32 = (AUDIO_OUT_RATE * 2 * 2 / 5) as i32;

struct Frontend {
    sdl: sdl3::Sdl,
    canvas: sdl3::render::WindowCanvas,
    creator: sdl3::render::TextureCreator<sdl3::video::WindowContext>,
    audio: Option<sdl3::audio::AudioStreamOwner>,
    /// Core samples -> AUDIO_OUT_RATE. The core's production rate follows
    /// the game's SOUNDBIAS resolution and CHANGES at runtime (BN4+ flip
    /// from 32768 to 65536 Hz after boot), so the SDL stream stays at one
    /// fixed rate and this tracks the moving source rate — same shape as
    /// tango's MGBAStream.
    resampler: mgba::audio::AudioResampler,
    resampled: mgba::audio::AudioBuffer,
    audio_scratch: Vec<i16>,
    /// Last core rate seen, to log transitions.
    audio_src_rate: u32,
    screens: usize,
}

impl Frontend {
    fn new(title: &str, screens: usize, mute: bool) -> Self {
        let sdl = sdl3::init().unwrap_or_else(|e| die(&format!("sdl init: {e}")));
        let video = sdl.video().unwrap_or_else(|e| die(&format!("sdl video: {e}")));
        let window = video
            .window(title, GBA_W * SCALE * screens as u32, GBA_H * SCALE)
            .position_centered()
            .build()
            .unwrap_or_else(|e| die(&format!("sdl window: {e}")));
        let canvas = window.into_canvas();
        let creator = canvas.texture_creator();

        let audio = if mute {
            None
        } else {
            match sdl.audio() {
                Ok(subsystem) => {
                    let spec = sdl3::audio::AudioSpec::new(
                        Some(AUDIO_OUT_RATE as i32),
                        Some(2),
                        Some(sdl3::audio::AudioFormat::S16LE),
                    );
                    match subsystem.default_playback_device().open_device_stream(Some(&spec)) {
                        Ok(stream) => {
                            let _ = stream.resume();
                            Some(stream)
                        }
                        Err(e) => {
                            eprintln!("audio disabled ({e})");
                            None
                        }
                    }
                }
                Err(e) => {
                    eprintln!("audio disabled ({e})");
                    None
                }
            }
        };

        Frontend {
            sdl,
            canvas,
            creator,
            audio,
            resampler: mgba::audio::AudioResampler::new(),
            // A tick is ~1100 frames at the highest GBA rate; rollback
            // bursts multiply that, and overflow just drops.
            resampled: mgba::audio::AudioBuffer::new(32768, 2),
            audio_scratch: Vec::new(),
            audio_src_rate: 0,
            screens,
        }
    }

    fn local_keys(pump: &sdl3::EventPump) -> u32 {
        use sdl3::keyboard::Scancode;
        let kb = pump.keyboard_state();
        let mut keys = 0u32;
        for (scancode, bit) in [
            (Scancode::X, 0),         // A
            (Scancode::Z, 1),         // B
            (Scancode::Backspace, 2), // Select
            (Scancode::Return, 3),    // Start
            (Scancode::Right, 4),
            (Scancode::Left, 5),
            (Scancode::Up, 6),
            (Scancode::Down, 7),
            (Scancode::S, 8), // R
            (Scancode::A, 9), // L
        ] {
            if kb.is_scancode_pressed(scancode) {
                keys |= 1 << bit;
            }
        }
        keys
    }

    fn draw(&mut self, pair: &Pair, order: &[usize]) {
        self.canvas.clear();
        // Streaming textures can't outlive a borrow of the creator across
        // frames without self-referential pain; at 240x160 re-creating each
        // frame is well under a millisecond and keeps this simple.
        for (slot, &core) in order.iter().take(self.screens).enumerate() {
            let Some(buf) = pair.video_buffer(core) else {
                continue;
            };
            let mut texture = self
                .creator
                .create_texture_streaming(sdl3::pixels::PixelFormat::XBGR1555, GBA_W, GBA_H)
                .unwrap_or_else(|e| die(&format!("texture: {e}")));
            texture.set_scale_mode(sdl3::render::ScaleMode::Nearest);
            texture
                .update(None, buf, (GBA_W * 2) as usize)
                .unwrap_or_else(|e| die(&format!("texture update: {e}")));
            let dst = sdl3::rect::Rect::new((slot as u32 * GBA_W * SCALE) as i32, 0, GBA_W * SCALE, GBA_H * SCALE);
            let _ = self.canvas.copy(&texture, None, Some(dst.into()));
        }
        self.canvas.present();
    }

    /// Resample whatever the local core produced this tick to the fixed
    /// output rate and hand it to SDL.
    fn pump_audio(&mut self, pair: &mut Pair, local: usize) {
        if self.audio.is_none() {
            return;
        }
        let mut core = pair.core_mut(local);
        let rate = core.as_ref().audio_sample_rate();
        if rate != self.audio_src_rate {
            if self.audio_src_rate != 0 {
                eprintln!("audio rate changed: {} -> {rate} Hz", self.audio_src_rate);
            }
            self.audio_src_rate = rate;
        }
        let mut source = core.audio_buffer();
        if source.available() == 0 {
            return;
        }
        self.resampler.set_source(&mut source, rate as f64, true);
        self.resampler
            .set_destination(&mut self.resampled, AUDIO_OUT_RATE as f64);
        self.resampler.process();

        let frames = self.resampled.available();
        if frames == 0 {
            return;
        }
        self.audio_scratch.resize(frames * 2, 0);
        let read = self.resampled.read(&mut self.audio_scratch, frames);
        if let Some(stream) = &self.audio {
            if stream.queued_bytes().unwrap_or(0) < AUDIO_MAX_QUEUED_BYTES {
                let _ = stream.put_data_i16(&self.audio_scratch[..read * 2]);
            }
        }
    }
}

// ---- main ----------------------------------------------------------------

fn main() {
    mgba::log::install_default_logger();
    let args = parse_args();

    let read_rom = |path: &str| match path {
        "testrom" => testrom::build(),
        path => std::fs::read(path).unwrap_or_else(|e| die(&format!("read {path}: {e}"))),
    };
    let rom = read_rom(args.rom.as_deref().unwrap());
    // Crossplay: the peer plays a different game, and we need our own copy
    // of it for their side of the pair (ROM bytes never cross the wire).
    let remote_rom = args.remote_rom.as_deref().map_or_else(|| rom.clone(), read_rom);
    let rom_crc = crc32fast::hash(&rom);
    let remote_rom_crc = crc32fast::hash(&remote_rom);
    let save = args
        .save
        .as_deref()
        .map(|p| std::fs::read(p).unwrap_or_else(|e| die(&format!("read {p}: {e}"))));

    let net = connect_transport(&args);
    let local_player = if net.is_player_0 { 0 } else { 1 };

    // Hello exchange over the reliable channel. Player 0 dictates the
    // match clock; the present delay is purely local now, so nothing
    // else needs agreeing on.
    let rtc_now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_micros() as u64;
    send_hello(&net, rom_crc, rtc_now, &save);
    let mut pending_data = Vec::new();
    let hello = recv_hello(&net, &mut pending_data);
    if hello.rom_crc != remote_rom_crc {
        die(&format!(
            "ROM mismatch: the peer is playing {:08x} but our copy of their game is {remote_rom_crc:08x} \
             — point --remote-rom at your copy of the game they're playing",
            hello.rom_crc
        ));
    }
    let rtc = if net.is_player_0 { rtc_now } else { hello.rtc };

    let (side0, side1) = if net.is_player_0 {
        (
            SideOptions { rom, save },
            SideOptions {
                rom: remote_rom,
                save: hello.save,
            },
        )
    } else {
        (
            SideOptions {
                rom: remote_rom,
                save: hello.save,
            },
            SideOptions { rom, save },
        )
    };
    let pair = Pair::with_options(PairOptions {
        sides: [side0, side1],
        rtc: Some(std::time::UNIX_EPOCH + std::time::Duration::from_micros(rtc)),
    })
    .unwrap_or_else(|e| die(&format!("pair boot: {e}")));

    let mut session =
        Session::new(pair, local_player, args.delay).unwrap_or_else(|e| die(&format!("session boot: {e}")));
    let mut throttler = Throttler::new();
    let mut out_stream = rennet::OutStream::<DemoProto>::new(HORIZON);
    let mut in_stream = rennet::InStream::<DemoProto>::new(HORIZON);

    let mut frontend = (!args.headless).then(|| {
        Frontend::new(
            &format!("mgba-siolink demo — player {}", local_player + 1),
            if args.show_remote { 2 } else { 1 },
            args.mute,
        )
    });
    let mut pump = frontend
        .as_mut()
        .map(|f| f.sdl.event_pump().unwrap_or_else(|e| die(&format!("event pump: {e}"))));
    // Local screen on the left when both are shown.
    let screen_order = [local_player, 1 - local_player];

    let mut next_tick = std::time::Instant::now();
    // Current fps shaved off the tick rate by the throttler; recomputed
    // on every advance, held across stalls.
    let mut slowdown = 0.0f32;
    let mut rollback_total = 0u64;
    let mut stalls = 0u64;
    let mut checkpoints_ok = 0u64;
    let mut peer_gone = false;
    let mut desynced = false;

    // rennet seq n carries the peer's input for their local tick n. The
    // session wants each remote input exactly once, in order; redundancy
    // re-deliveries below this watermark are dropped here.
    let mut next_remote_seq = 0u32;

    let ingest_frame = |bytes: &[u8],
                        session: &mut Session,
                        out_stream: &mut rennet::OutStream<DemoProto>,
                        in_stream: &mut rennet::InStream<DemoProto>,
                        next_remote_seq: &mut u32|
     -> Result<(), String> {
        let frame = rennet::Frame::<DemoProto>::decode(&mut &bytes[..]).map_err(|e| format!("bad frame: {e}"))?;
        out_stream.apply_ack(frame.ack());
        let window = in_stream
            .accept(&frame)
            .map_err(|_| "peer ran past the rollback horizon".to_owned())?;
        for (i, keys) in window.entries.iter().enumerate() {
            if window.base + i as u32 == *next_remote_seq {
                session.add_remote_input(keys.0 as u32, window.meta.0);
                *next_remote_seq += 1;
            }
        }
        Ok(())
    };

    for frame in std::mem::take(&mut pending_data) {
        if let Err(e) = ingest_frame(
            &frame,
            &mut session,
            &mut out_stream,
            &mut in_stream,
            &mut next_remote_seq,
        ) {
            die(&e);
        }
    }

    'main: loop {
        let mut keys = if args.wiggle {
            // Changes every few frames so repeat-last prediction misses.
            ((session.frontier() / 5).wrapping_mul(2654435761) >> local_player) & 0x3ff
        } else {
            0u32
        };
        if let Some(pump) = pump.as_mut() {
            for event in pump.poll_iter() {
                use sdl3::event::Event;
                match event {
                    Event::Quit { .. } => {
                        eprintln!("window quit event");
                        break 'main;
                    }
                    Event::KeyDown {
                        scancode: Some(sdl3::keyboard::Scancode::Escape),
                        ..
                    } => break 'main,
                    _ => {}
                }
            }
            keys |= Frontend::local_keys(pump);
        }

        // Drain the network before advancing: every confirmed tick we
        // ingest now is a rollback we don't take deeper.
        for incoming in net.incoming.try_iter() {
            match incoming {
                Incoming::Data(bytes) => {
                    if let Err(e) = ingest_frame(
                        &bytes,
                        &mut session,
                        &mut out_stream,
                        &mut in_stream,
                        &mut next_remote_seq,
                    ) {
                        eprintln!("fatal: {e}");
                        desynced = true;
                        break 'main;
                    }
                }
                Incoming::Control(msg) => match msg.first() {
                    Some(&MSG_CHECKPOINT) if msg.len() == 9 => {
                        let tick = u32::from_le_bytes(msg[1..5].try_into().unwrap());
                        let digest = u32::from_le_bytes(msg[5..9].try_into().unwrap());
                        match session.digest_at(tick) {
                            Some(ours) if ours != digest => {
                                eprintln!("DESYNC at tick {tick}: ours {ours:08x}, peer's {digest:08x}");
                                desynced = true;
                                break 'main;
                            }
                            Some(_) => checkpoints_ok += 1,
                            None => {}
                        }
                    }
                    Some(&MSG_BYE) => {
                        eprintln!("peer left");
                        peer_gone = true;
                        break 'main;
                    }
                    _ => {
                        eprintln!("unexpected control message");
                    }
                },
                Incoming::Gone(why) => {
                    eprintln!("peer connection lost: {why}");
                    peer_gone = true;
                    break 'main;
                }
            }
        }

        // Advance one tick — unless the unconfirmed backlog has grown so
        // deep that the peer is clearly far behind (or gone), or our
        // unacked rennet window is about to shed elements the peer never
        // got. (Small leads don't stall — the throttler below sheds fps
        // to let the trailing peer catch up gracefully.)
        let unconfirmed = session.local_queue_length() as u32;
        let unacked = out_stream.next_seq().saturating_sub(out_stream.peer_ack_base());
        let advanced = if unconfirmed >= HORIZON - STALL_MARGIN || unacked >= HORIZON - STALL_MARGIN {
            stalls += 1;
            // Keep acks + the redundancy window flowing even while stalled —
            // this is exactly how the peer recovers if loss is what got us
            // here.
            let w = out_stream.window();
            let heartbeat = rennet::Frame::<DemoProto>::new(w.base, in_stream.ack(), w.meta, w.entries);
            let _ = net.data_tx.send(heartbeat.to_vec());
            false
        } else {
            // Sample the skew before `advance` enqueues this tick's local
            // input, so our half matches the advantage we ship the peer.
            let skew = session.skew();
            match session.advance(keys) {
                Ok((outgoing, report)) => {
                    debug_assert_eq!(out_stream.next_seq(), outgoing.tick);
                    out_stream.push_with_meta(Keys(outgoing.keys as u16), Adv(outgoing.tick_advantage));
                    let w = out_stream.window();
                    let frame = rennet::Frame::<DemoProto>::new(w.base, in_stream.ack(), w.meta, w.entries);
                    let _ = net.data_tx.send(frame.to_vec());
                    rollback_total += u64::from(report.rolled_back);
                    // Clock sync: only the leading peer shaves tick rate,
                    // and only once the presented frame actually
                    // speculates past the present delay.
                    slowdown = throttler.step(skew, session.speculation_balance());
                    if report.frontier % CHECKPOINT_EVERY == 0 {
                        if let Some((tick, digest)) = session.checkpoint() {
                            let mut msg = vec![MSG_CHECKPOINT];
                            msg.extend_from_slice(&tick.to_le_bytes());
                            msg.extend_from_slice(&digest.to_le_bytes());
                            let _ = net.control_tx.send(msg);
                        }
                    }
                    true
                }
                Err(e) => {
                    eprintln!("fatal: {e}");
                    desynced = true;
                    break 'main;
                }
            }
        };

        if let Some(f) = frontend.as_mut() {
            // The pair is parked at the newest simulated tick, which in
            // steady state is exactly the presented tick (`frontier -
            // present_delay`) — the engine never simulates past it.
            session.with_pair(|pair| {
                f.draw(pair, &screen_order);
                if advanced {
                    f.pump_audio(pair, local_player);
                }
            });
        }

        if let Some(max) = args.frames {
            if session.frontier() >= max {
                break;
            }
        }

        // Pace at the base rate minus whatever the throttler shaved: the
        // leading peer runs slightly slow until the clocks realign.
        next_tick += std::time::Duration::from_secs_f64(1.0 / (GBA_FPS - slowdown as f64));
        let now = std::time::Instant::now();
        if next_tick > now {
            std::thread::sleep(next_tick - now);
        } else if now - next_tick > std::time::Duration::from_millis(250) {
            // Fell way behind (debugger, laptop lid, ...): don't sprint to
            // catch up, just resynchronize the cadence.
            next_tick = now;
        }
    }

    let _ = net.control_tx.send(vec![MSG_BYE]);
    // Give the writer task a beat to flush the bye before the runtime drops.
    std::thread::sleep(std::time::Duration::from_millis(50));
    eprintln!(
        "done: {} ticks simulated, {} confirmed, {} rolled back, {} stalls, {} checkpoints verified{}{}",
        session.frontier(),
        session.confirmed(),
        rollback_total,
        stalls,
        checkpoints_ok,
        if peer_gone { ", peer gone" } else { "" },
        if desynced { ", DESYNCED" } else { "" },
    );
    std::process::exit(if desynced { 2 } else { 0 });
}
