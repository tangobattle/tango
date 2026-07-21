//! The PvP session, browser flavor: `tango_pvp::engine::Match` (the
//! same SIO-lockstep pair + gamesupport priming + telemetry as the
//! desktop) ticked by the runtime pump, with the desktop drive loop's
//! per-frame body — drain the network, stall-guard against the rennet
//! horizon, read skew before advance, advance, ship the input, fold
//! confirmed ticks into the replay, throttle — reshaped onto gbaroll's
//! proven single-thread pump/driver split.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use futures::channel::mpsc;
use tango_net_protocol::data::{Element, Meta, RECONNECT_QUEUE_LENGTH};
use tango_pvp::Throttler;
use web_time::Instant;

use crate::library::{self, GameRef};
use crate::net::data::{self, InMatchTx, NetEvent};
use crate::netplay::PreMatch;
use crate::session::{
    LinkAccess, SessionDescriptor, SessionEnd, SessionKind, SharedSession, EXPECTED_FPS,
};

/// How long `tick` waits for the peer's EndOfMatch after local
/// completion before ending anyway — the desktop's PEER_END_GRACE.
const PEER_END_GRACE_MS: u64 = 5_000;

pub struct PvpArgs {
    pub pre: PreMatch,
    pub local_rom: Vec<u8>,
    pub remote_rom: Vec<u8>,
    pub present_delay: u32,
}

pub struct PvpSession {
    pub driver: PvpDriver,
    pub shared: Arc<SharedSession>,
    pub link: LinkAccess,
    pub descriptor: SessionDescriptor,
}

/// Boot + prime the pair (synchronously — a one-time multi-second
/// stall behind the "match starting" line) and spawn the transport
/// pumps.
pub fn start(args: PvpArgs) -> anyhow::Result<PvpSession> {
    let PvpArgs {
        pre,
        local_rom,
        remote_rom,
        present_delay,
    } = args;

    // Both peers derive the same player assignment from the shared
    // seed; core 0 always runs player 0's game on both peers.
    let mut rng = {
        use rand::SeedableRng;
        rand_pcg::Mcg128Xsl64::from_seed(pre.rng_seed)
    };
    let local_player =
        tango_net_protocol::derive::pick_local_player_index(&mut rng, pre.is_offerer) as usize;

    let (roms, saves, games): ([Vec<u8>; 2], [Vec<u8>; 2], [GameRef; 2]) = if local_player == 0 {
        (
            [local_rom, remote_rom],
            [pre.local_save.clone(), pre.remote_save.clone()],
            [pre.local_game, pre.remote_game],
        )
    } else {
        (
            [remote_rom, local_rom],
            [pre.remote_save.clone(), pre.local_save.clone()],
            [pre.remote_game, pre.local_game],
        )
    };

    let rtc = std::time::UNIX_EPOCH + std::time::Duration::from_millis(pre.match_ts);
    let match_ = tango_pvp::engine::Match::new(tango_pvp::engine::MatchConfig {
        roms,
        saves,
        support: [games[0].pvp, games[1].pvp],
        match_type: pre.match_type,
        rng_seed: pre.rng_seed,
        rtc,
        local_player,
        present_delay,
        disable_bgm: false,
    })?;

    // Everything that borrows `pre` happens before the channel/pc
    // moves below.
    let replay_buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let replay_writer = build_replay_writer(&pre, local_player, replay_buf.clone())?;
    let replay_name = replay_file_name(&pre);

    let shared = SharedSession::new(present_delay);
    shared.view_player.store(local_player, Ordering::Relaxed);

    let stop = Rc::new(Cell::new(false));
    let (in_match, event_rx) = data::wire(pre.in_match_tx, pre.in_match_rx, stop.clone());

    // The reliable channel's only mid-match job is the peer's Goodbye
    // (deliberate quit) vs a bare close (reconnectable).
    let (goodbye_tx, goodbye_rx) = mpsc::unbounded::<bool>();
    spawn_goodbye_watch(pre.control_rx, goodbye_tx.clone());

    let link_handle = match_.pair_handle();
    let descriptor = SessionDescriptor {
        kind: SessionKind::Pvp,
        local_player,
        game: pre.local_game,
    };

    let driver = PvpDriver {
        shared: shared.clone(),
        local_player,
        match_,
        throttler: Throttler::new(),
        in_match,
        event_rx,
        goodbye_rx,
        goodbye_tx,
        reconnect_session_id: pre.reconnect_session_id,
        reconnecting: None,
        control_tx: pre.control_tx,
        pc: Some(pre.pc),
        stop,
        completed: false,
        fired_end_of_match: false,
        peer_ended: false,
        local_ended_at: None,
        first_round_started: false,
        pending_round_marks: std::collections::VecDeque::new(),
        round_wins: [0, 0],
        round_draws: 0,
        replay_writer: Some(replay_writer),
        replay_buf,
        replay_name,
        tick_times: std::collections::VecDeque::new(),
    };

    Ok(PvpSession {
        driver,
        shared,
        link: LinkAccess::Handle(link_handle),
        descriptor,
    })
}

/// Watch the reliable channel for the peer's Goodbye (true) or a bare
/// close (false — reconnectable).
fn spawn_goodbye_watch(
    mut rx: crate::net::control::Receiver,
    goodbye_tx: mpsc::UnboundedSender<bool>,
) {
    wasm_bindgen_futures::spawn_local(async move {
        loop {
            match rx.receive().await {
                Ok(tango_net_protocol::control::Packet::Goodbye(_)) => {
                    let _ = goodbye_tx.unbounded_send(true);
                    return;
                }
                Ok(_) => continue,
                Err(_) => {
                    let _ = goodbye_tx.unbounded_send(false);
                    return;
                }
            }
        }
    });
}

fn build_replay_writer(
    pre: &PreMatch,
    local_player: usize,
    buf: Arc<Mutex<Vec<u8>>>,
) -> anyhow::Result<tango_pvp::replay::Writer> {
    use tango_pvp::replay::metadata;

    struct SharedBuf(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for SharedBuf {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let side = |settings: &tango_net_protocol::control::Settings| {
        let game_info = settings.game_info.as_ref();
        metadata::Side {
            nickname: settings.nickname.clone(),
            game_info: game_info.map(|g| metadata::GameInfo {
                rom_family: g.family_and_variant.0.clone(),
                rom_variant: g.family_and_variant.1 as u32,
                patch: g.patch.as_ref().map(|p| metadata::game_info::Patch {
                    name: p.name.clone(),
                    version: p.version.to_string(),
                }),
            }),
            reveal_setup: !settings.blind_setup,
            client_cert_fingerprint_sha256: vec![],
        }
    };

    let metadata = tango_pvp::replay::Metadata {
        ts: pre.match_ts,
        link_code: String::new(),
        local_side: Some(side(&pre.local_settings)),
        remote_side: Some(side(&pre.remote_settings)),
        match_type: pre.match_type.0 as u32,
        match_subtype: pre.match_type.1 as u32,
    };

    Ok(tango_pvp::replay::Writer::new(
        SharedBuf(buf),
        tango_pvp::replay::VERSION,
        metadata,
        pre.is_offerer,
        local_player as u8,
        pre.rng_seed,
        &pre.local_save,
        &pre.remote_save,
    )?)
}

fn replay_file_name(pre: &PreMatch) -> String {
    let (family, _) = pre.local_game.family_and_variant();
    let opponent: String = pre
        .remote_settings
        .nickname
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(24)
        .collect();
    format!("{}-{}-vs-{}.tangoreplay", pre.match_ts, family, opponent)
}

/// A successful reconnect's fresh transport halves, produced by the
/// rendezvous task for the driver to swap in.
struct FreshTransport {
    control_tx: crate::net::control::Sender,
    control_rx: crate::net::control::Receiver,
    in_match_tx: crate::net::webrtc::ChannelSender,
    in_match_rx: crate::net::webrtc::ChannelReceiver,
    pc: crate::net::webrtc::PeerConnection,
}

/// An in-flight reconnect attempt.
struct ReconnectAttempt {
    started: Instant,
    result_rx: mpsc::UnboundedReceiver<Result<FreshTransport, String>>,
}

/// Overall budget for one transparent reconnect (rendezvous + ICE).
const RECONNECT_WINDOW_MS: u64 = 30_000;

pub struct PvpDriver {
    shared: Arc<SharedSession>,
    local_player: usize,
    match_: tango_pvp::engine::Match,
    throttler: Throttler,
    in_match: InMatchTx,
    event_rx: mpsc::UnboundedReceiver<NetEvent>,
    goodbye_rx: mpsc::UnboundedReceiver<bool>,
    goodbye_tx: mpsc::UnboundedSender<bool>,
    reconnect_session_id: String,
    reconnecting: Option<ReconnectAttempt>,
    control_tx: crate::net::control::Sender,
    pc: Option<crate::net::webrtc::PeerConnection>,
    stop: Rc<Cell<bool>>,
    completed: bool,
    fired_end_of_match: bool,
    peer_ended: bool,
    local_ended_at: Option<Instant>,
    first_round_started: bool,
    pending_round_marks: std::collections::VecDeque<u32>,
    /// Confirmed round outcomes in absolute player terms: [p0, p1]
    /// wins and the draw count, reoriented at the end.
    round_wins: [u32; 2],
    round_draws: u32,
    replay_writer: Option<tango_pvp::replay::Writer>,
    replay_buf: Arc<Mutex<Vec<u8>>>,
    replay_name: String,
    tick_times: std::collections::VecDeque<Instant>,
}

impl PvpDriver {
    pub fn tick(&mut self) -> bool {
        if let Some(end) = self.tick_inner() {
            self.finish(end);
            return false;
        }
        true
    }

    fn tick_inner(&mut self) -> Option<SessionEnd> {
        if self.shared.quit.load(Ordering::Relaxed) {
            return Some(SessionEnd::LocalQuit);
        }

        // Peer's deliberate quit / a dead control channel. A bare close
        // during (or starting) a reconnect is expected static.
        if let Ok(deliberate) = self.goodbye_rx.try_recv() {
            if deliberate || self.match_ended() {
                return Some(self.match_ended_result());
            }
            if self.reconnecting.is_none() {
                self.begin_reconnect();
            }
        }

        // An in-flight reconnect: poll its outcome; time the window out.
        if let Some(attempt) = &mut self.reconnecting {
            match attempt.result_rx.try_recv() {
                Ok(Ok(fresh)) => {
                    log::info!("pvp: transport reconnected; resuming");
                    self.control_tx = fresh.control_tx;
                    spawn_goodbye_watch(fresh.control_rx, self.goodbye_tx.clone());
                    self.in_match.swap_transport(fresh.in_match_tx, fresh.in_match_rx);
                    self.pc = Some(fresh.pc);
                    self.reconnecting = None;
                }
                Ok(Err(e)) => {
                    return Some(SessionEnd::Error(format!("reconnect failed: {e}")));
                }
                Err(_) => {
                    if attempt.started.elapsed().as_millis() as u64 >= RECONNECT_WINDOW_MS {
                        return Some(SessionEnd::Error("connection lost".to_string()));
                    }
                }
            }
        }

        let pd = self.shared.present_delay.load(Ordering::Relaxed);
        if pd != self.match_.present_delay() {
            self.match_.set_present_delay(pd);
        }

        // Drain the network before advancing: every confirmed tick we
        // ingest now is a rollback we don't take deeper.
        while let Ok(ev) = self.event_rx.try_recv() {
            match ev {
                NetEvent::Input(input) => {
                    self.match_
                        .add_remote_input(input.joyflags as u32, input.tick_advantage);
                }
                NetEvent::EndOfMatch => {
                    self.peer_ended = true;
                }
                NetEvent::Gone { generation } => {
                    if generation < self.in_match.generation() {
                        // A stale pump dying after a swap.
                        continue;
                    }
                    if self.match_ended() {
                        return Some(self.match_ended_result());
                    }
                    if self.reconnecting.is_none() {
                        self.begin_reconnect();
                    }
                }
            }
        }

        // Completion gate (the desktop's `is_ended`): the games' own
        // match-end anchor fired locally, and the peer ended too (or
        // the grace ran out).
        if self.completed {
            let grace_over = self
                .local_ended_at
                .is_some_and(|at| at.elapsed().as_millis() as u64 >= PEER_END_GRACE_MS);
            if self.peer_ended || grace_over {
                return Some(self.match_ended_result());
            }
        }

        // Stall guard: hold only when advancing can't make progress
        // (see the desktop drive loop for the full rationale — resend
        // bursts must keep settling even at a full queue).
        let queue_len = self.match_.local_queue_length();
        if queue_len >= RECONNECT_QUEUE_LENGTH && self.match_.matchable() == 0 {
            // The pump re-ticks us; nothing to do this frame. Transparent
            // reconnect is the M5 refinement — a long enough stall ends
            // via the peer-gone path above.
            return None;
        }

        // Sample the skew before `advance` enqueues this tick's local
        // input, so our half matches the advantage we ship the peer.
        let skew = self.match_.skew();

        let keys = self.shared.joyflags.load(Ordering::Relaxed)
            & tango_pvp::input::JOYFLAGS_MASK as u32;
        let (outgoing, report) = match self.match_.advance(keys) {
            Ok(v) => v,
            Err(e) => return Some(SessionEnd::Error(format!("emulation error: {e}"))),
        };

        // Ship this tick's local input (the whole redundancy window
        // rides along; the heartbeat covers idle intervals).
        self.in_match.ship(
            Element::Input(outgoing.keys as u16),
            Meta {
                tick_advantage: outgoing.tick_advantage,
            },
        );

        // Confirmed telemetry first, so this batch's round starts can
        // stamp markers onto this batch's replay records.
        let (_samples, events) = self
            .match_
            .telemetry()
            .lock()
            .unwrap()
            .drain_confirmed(report.confirmed);
        let mut match_ended = false;
        for (tick, event) in &events {
            match event {
                tango_pvp::telemetry::RoundEvent::Started => {
                    if self.first_round_started {
                        self.pending_round_marks.push_back(*tick);
                    }
                    self.first_round_started = true;
                }
                tango_pvp::telemetry::RoundEvent::Ended { outcome } => match outcome {
                    Some(tango_pvp::telemetry::Outcome::P0Win) => self.round_wins[0] += 1,
                    Some(tango_pvp::telemetry::Outcome::P1Win) => self.round_wins[1] += 1,
                    Some(tango_pvp::telemetry::Outcome::Draw) | None => self.round_draws += 1,
                },
                tango_pvp::telemetry::RoundEvent::MatchEnded => {
                    match_ended = true;
                }
            }
        }

        // Confirmed inputs into the replay.
        let confirmed_inputs = self.match_.drain_confirmed();
        if let Some(w) = self.replay_writer.as_mut() {
            for (tick, keys) in &confirmed_inputs {
                if self.pending_round_marks.front().is_some_and(|t| t <= tick) {
                    self.pending_round_marks.pop_front();
                    let _ = w.start_round();
                }
                let (p0, p1) = (keys[0] as u16, keys[1] as u16);
                let (local, remote) = if self.local_player == 0 {
                    (p0, p1)
                } else {
                    (p1, p0)
                };
                if let Err(e) = w.write_input(
                    self.local_player as u8,
                    &(
                        tango_pvp::input::Input { joyflags: local },
                        tango_pvp::input::Input { joyflags: remote },
                    ),
                ) {
                    log::warn!("pvp: replay write failed (recording stops): {e}");
                    self.replay_writer = None;
                    break;
                }
            }
        }

        // Completion: the games' own match-end path ran (confirmed —
        // both peers see it at the same pair tick).
        if match_ended {
            self.completed = true;
        }
        if self.completed && !self.fired_end_of_match {
            self.fired_end_of_match = true;
            self.local_ended_at = Some(Instant::now());
            // In-band EndOfMatch: rides the same ordered seq stream as
            // inputs, so the peer sees it exactly once and only after
            // every preceding input.
            self.in_match.ship(
                Element::EndOfMatch,
                Meta {
                    tick_advantage: 0,
                },
            );
        }

        // Present the local screen.
        if let Some(buf) = self.match_.local_video_buffer() {
            self.shared.publish_video(&buf);
        }

        // Clock sync: only the leading peer shaves tick rate.
        let slowdown = self.throttler.step(skew, self.match_.speculation_balance());
        let fps_target = EXPECTED_FPS - slowdown;
        self.shared.set_fps_target(fps_target);

        // Measured TPS + stats for the HUD.
        let now = Instant::now();
        self.tick_times.push_back(now);
        while self
            .tick_times
            .front()
            .is_some_and(|t| now.duration_since(*t) > web_time::Duration::from_secs(1))
        {
            self.tick_times.pop_front();
        }
        {
            let mut stats = self.shared.stats.lock().unwrap();
            stats.queue_len = queue_len as u32;
            stats.skew = skew;
            stats.rolled_back = report.rolled_back;
            stats.confirmed = report.confirmed;
            stats.frontier = report.frontier;
            stats.slices_peak = report.slices_peak;
            stats.tps = self.tick_times.len() as f32;
            stats.fps_target = fps_target;
            stats.rtt_ms = self.in_match.rtt_ms();
        }

        None
    }

    fn match_ended(&self) -> bool {
        self.completed && self.peer_ended
    }

    /// The local-oriented final tally.
    fn match_ended_result(&self) -> SessionEnd {
        let (wins, losses) = if self.local_player == 0 {
            (self.round_wins[0], self.round_wins[1])
        } else {
            (self.round_wins[1], self.round_wins[0])
        };
        SessionEnd::MatchEnded {
            wins,
            losses,
            draws: self.round_draws,
        }
    }

    /// Kick a transparent reconnect: both peers re-rendezvous on the
    /// derived session id (the desktop does the same on its side) and
    /// the fresh channel halves swap in under the surviving rennet
    /// streams. The sim keeps ticking meanwhile — the stall guard holds
    /// it at the horizon edge until inputs flow again.
    fn begin_reconnect(&mut self) {
        log::warn!("pvp: transport lost; attempting transparent reconnect");
        let (result_tx, result_rx) = mpsc::unbounded();
        let session_id = self.reconnect_session_id.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let endpoint = crate::config::matchmaking_endpoint();
            let result = crate::net::signaling::connect(&endpoint, &session_id, None).await;
            let _ = result_tx.unbounded_send(match result {
                Ok(connected) => {
                    // The channel-open barrier still applies to the
                    // fresh control channel.
                    match connected.control_open.await {
                        Ok(()) => Ok(FreshTransport {
                            control_tx: crate::net::control::Sender::new(connected.control_tx),
                            control_rx: crate::net::control::Receiver::new(connected.control_rx),
                            in_match_tx: connected.in_match_tx,
                            in_match_rx: connected.in_match_rx,
                            pc: connected.pc,
                        }),
                        Err(_) => Err("control channel never opened".to_string()),
                    }
                }
                Err(e) => Err(format!("{e}")),
            });
        });
        self.reconnecting = Some(ReconnectAttempt {
            started: Instant::now(),
            result_rx,
        });
    }

    /// Teardown: stop the pumps, announce a deliberate quit, flush the
    /// replay to OPFS, close the transport once its buffers drain.
    fn finish(&mut self, end: SessionEnd) {
        self.stop.set(true);

        if matches!(end, SessionEnd::LocalQuit) {
            let _ = self.control_tx.send_goodbye();
        }

        // Flush the replay tail; finalize (EOR sentinel) only on a
        // completed match — an abort leaves a truncated-but-parseable
        // recording, the desktop's policy.
        if let Some(mut w) = self.replay_writer.take() {
            for (_, keys) in self.match_.drain_confirmed() {
                let (p0, p1) = (keys[0] as u16, keys[1] as u16);
                let (local, remote) = if self.local_player == 0 {
                    (p0, p1)
                } else {
                    (p1, p0)
                };
                let _ = w.write_input(
                    self.local_player as u8,
                    &(
                        tango_pvp::input::Input { joyflags: local },
                        tango_pvp::input::Input { joyflags: remote },
                    ),
                );
            }
            if self.completed {
                if let Err(e) = w.finish() {
                    log::error!("finish replay failed: {e}");
                }
            }
        }
        let bytes = std::mem::take(&mut *self.replay_buf.lock().unwrap());
        if !bytes.is_empty() {
            let name = self.replay_name.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let Ok(storage) = crate::storage::Storage::open().await else {
                    return;
                };
                match crate::storage::write(storage.replays(), &name, &bytes).await {
                    Ok(()) => log::info!("replay saved: {name} ({} bytes)", bytes.len()),
                    Err(e) => log::error!("couldn't save replay {name}: {e}"),
                }
            });
        }

        // Close the peer connection once the control channel drains.
        if let Some(pc) = self.pc.take() {
            wasm_bindgen_futures::spawn_local(async move {
                gloo_timers::future::TimeoutFuture::new(200).await;
                pc.close();
                drop(pc);
            });
        }

        self.shared.finish(end);
    }
}

/// Boot a PvP session from the lobby handoff: resolve both ROMs from
/// the library, then build + prime the pair.
pub async fn boot_from_handoff(
    runtime: std::rc::Rc<std::cell::RefCell<crate::runtime::Runtime>>,
    storage: crate::storage::Storage,
    lib: library::Library,
) -> anyhow::Result<()> {
    let pre = crate::netplay::PRE_MATCH
        .with(|slot| slot.borrow_mut().take())
        .ok_or_else(|| anyhow::anyhow!("no pre-match handoff"))?;

    let read_rom = |game: GameRef| {
        let entry = lib
            .by_game(game)
            .ok_or_else(|| anyhow::anyhow!("{}'s ROM isn't imported", library::display_name(game)))
            .map(|e| e.file.clone());
        entry
    };
    let local_file = read_rom(pre.local_game)?;
    let remote_file = read_rom(pre.remote_game)?;
    let local_rom = crate::storage::read(storage.roms(), &local_file)
        .await
        .map_err(|e| anyhow::anyhow!("read local rom: {e}"))?
        .ok_or_else(|| anyhow::anyhow!("local ROM disappeared"))?;
    let remote_rom = crate::storage::read(storage.roms(), &remote_file)
        .await
        .map_err(|e| anyhow::anyhow!("read remote rom: {e}"))?
        .ok_or_else(|| anyhow::anyhow!("remote ROM disappeared"))?;

    let present_delay = crate::config::Config::load().present_delay;
    runtime
        .borrow_mut()
        .start_pvp(crate::session::pvp::PvpArgs {
            pre,
            local_rom,
            remote_rom,
            present_delay,
        })
}
