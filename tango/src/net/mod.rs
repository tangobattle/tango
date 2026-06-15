//! Per-peer netplay transport: a Sender + Receiver pair backed by a
//! pluggable message-oriented transport (`PacketSink` / `PacketStream`),
//! plus `negotiate()` (exchange Hellos and check the protocol versions
//! agree).
//!
//! The WebRTC DataChannel impl lives in [`datachannel`]; the
//! transport-agnostic framing and packet helpers live here.
//!
//! `PvpSender` / `PvpReceiver` (the `tango_pvp::net` adapters used by
//! the live battle loop) come in a later netplay round.

pub mod datachannel;
pub mod protocol;
pub mod tcp;
pub mod stream;
pub mod udp;
pub mod wire;

/// Default port for the direct-TCP local-play transport (link-code
/// commands `/host` and `/connect`). `24680` reads as a memorable
/// even-step sequence and steers clear of every well-known service
/// in the ephemeral range — easy to type, easy to recite over voice
/// chat, unlikely to clash with anything already listening locally.
pub const DEFAULT_LOCAL_PORT: u16 = 24680;

/// One half of a peer connection's send side. Carries discrete,
/// reliable, in-order byte messages — same contract as a WebRTC
/// DataChannel configured `unordered: false, unreliable: false`. A
/// TCP-backed impl must add its own length-prefix framing so each
/// `send` round-trips as exactly one `recv` on the peer.
#[async_trait::async_trait]
pub trait PacketSink: Send + Sync {
    async fn send(&mut self, bytes: &[u8]) -> std::io::Result<()>;
}

/// One half of a peer connection's receive side. See [`PacketSink`]
/// for the contract on message boundaries. A clean stream close is
/// reported as `io::ErrorKind::UnexpectedEof`.
#[async_trait::async_trait]
pub trait PacketStream: Send + Sync {
    async fn recv(&mut self) -> std::io::Result<Vec<u8>>;
}

/// How often the lobby + match loops fire a ping. Latency is computed
/// from the matching Pong; absent pongs after a few intervals signal
/// a dropped peer.
pub const PING_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

#[derive(Debug, thiserror::Error)]
pub enum NegotiationError {
    #[error("expected hello")]
    ExpectedHello,
    #[error("remote protocol version too old")]
    RemoteProtocolVersionTooOld,
    #[error("remote protocol version too new")]
    RemoteProtocolVersionTooNew,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// Exchange Hello packets with the peer and verify both sides speak
/// the same `protocol::VERSION`. Has to run on both peers before any
/// other packet is sent.
pub async fn negotiate(sender: &mut Sender, receiver: &mut Receiver) -> Result<(), NegotiationError> {
    sender
        .send_hello()
        .await
        .map_err(|e| NegotiationError::Other(e.into()))?;
    let hello = match receiver.receive().await.map_err(|_| NegotiationError::ExpectedHello)? {
        protocol::Packet::Hello(h) => h,
        _ => return Err(NegotiationError::ExpectedHello),
    };
    if hello.protocol_version < protocol::VERSION {
        return Err(NegotiationError::RemoteProtocolVersionTooOld);
    }
    if hello.protocol_version > protocol::VERSION {
        return Err(NegotiationError::RemoteProtocolVersionTooNew);
    }
    Ok(())
}

pub struct Sender {
    sink: Box<dyn PacketSink>,
}

impl Sender {
    pub fn new(sink: Box<dyn PacketSink>) -> Self {
        Self { sink }
    }

    pub async fn send_packet(&mut self, p: &protocol::Packet) -> std::io::Result<()> {
        self.sink.send(p.serialize().unwrap().as_slice()).await
    }

    pub async fn send_hello(&mut self) -> std::io::Result<()> {
        self.send_packet(&protocol::Packet::Hello(protocol::Hello {
            protocol_version: protocol::VERSION,
        }))
        .await
    }

    pub async fn send_ping(&mut self, ts: u16) -> std::io::Result<()> {
        self.send_packet(&protocol::Packet::Ping(protocol::Ping { ts })).await
    }

    pub async fn send_pong(&mut self, ts: u16) -> std::io::Result<()> {
        self.send_packet(&protocol::Packet::Pong(protocol::Pong { ts })).await
    }

    pub async fn send_settings(&mut self, settings: protocol::Settings) -> std::io::Result<()> {
        self.send_packet(&protocol::Packet::Settings(settings)).await
    }

    pub async fn send_commit(&mut self, commitment: [u8; 16]) -> std::io::Result<()> {
        self.send_packet(&protocol::Packet::Commit(protocol::Commit { commitment }))
            .await
    }

    pub async fn send_uncommit(&mut self) -> std::io::Result<()> {
        self.send_packet(&protocol::Packet::Uncommit(protocol::Uncommit {}))
            .await
    }

    pub async fn send_chunk(&mut self, chunk: Vec<u8>) -> std::io::Result<()> {
        self.send_packet(&protocol::Packet::Chunk(protocol::Chunk { chunk }))
            .await
    }

    pub async fn send_start_match(&mut self) -> std::io::Result<()> {
        self.send_packet(&protocol::Packet::StartMatch(protocol::StartMatch {}))
            .await
    }

    // EndOfRound / EndOfMatch are no longer reliable-channel packets — they ride
    // in-band as `wire` markers on the unreliable in-match channel (see
    // `InMatchTx`), so the old `send_end_of_round` / `send_end_of_match` helpers
    // are gone.

    /// Ship a pre-serialized payload straight to the transport, bypassing the
    /// `Packet` framing. The in-match [`wire`] protocol owns its own framing
    /// (it runs over the unreliable WebRTC channel, or shares the reliable
    /// channel on the direct-TCP path), so it writes its bytes through here.
    pub async fn send_raw(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.sink.send(bytes).await
    }
}

pub struct Receiver {
    stream: Box<dyn PacketStream>,
}

impl Receiver {
    pub fn new(stream: Box<dyn PacketStream>) -> Self {
        Self { stream }
    }

    pub async fn receive(&mut self) -> std::io::Result<protocol::Packet> {
        let bytes = self.stream.recv().await?;
        protocol::Packet::deserialize(bytes.as_slice())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Read one raw transport message without `Packet` decoding — the receive
    /// counterpart to [`Sender::send_raw`], used by the in-match [`wire`]
    /// reader. A clean close surfaces as `UnexpectedEof`.
    pub async fn recv_raw(&mut self) -> std::io::Result<Vec<u8>> {
        self.stream.recv().await
    }
}

/// Median-of-window latency tracker. Identical to the legacy
/// `tango/src/stats.rs::LatencyCounter` — used by the PvP loop
/// to report ping in the running match.
#[derive(Clone)]
pub struct LatencyCounter {
    marks: std::collections::VecDeque<std::time::Duration>,
    window_size: usize,
}

impl LatencyCounter {
    pub fn new(window_size: usize) -> Self {
        Self {
            marks: std::collections::VecDeque::with_capacity(window_size),
            window_size,
        }
    }

    pub fn mark(&mut self, d: std::time::Duration) {
        while self.marks.len() >= self.window_size {
            self.marks.pop_front();
        }
        self.marks.push_back(d);
    }

    pub fn median(&self) -> std::time::Duration {
        if self.marks.is_empty() {
            return std::time::Duration::ZERO;
        }
        let mut marks = self.marks.iter().collect::<Vec<_>>();
        let (_, v, _) = marks.select_nth_unstable(self.marks.len() / 2);
        **v
    }

    /// Most recent (raw) ping mark — the latest single measurement, with no
    /// smoothing. `None` before the first `mark` (so callers can tell "no
    /// reading yet" from a genuine 0 ms ping). Feeds the live latency readout,
    /// where the median's lag would hide a real spike; [`median`](Self::median)
    /// stays the source for the frame-delay suggestion, which wants it smoothed.
    pub fn latest(&self) -> Option<std::time::Duration> {
        self.marks.back().copied()
    }
}

/// Send-pump queue depth. Deeper than the engine's unacked-local-input cap
/// so that under a genuinely stalled wire the engine's overflow bail fires
/// before the pump's channel ever blocks the frame — backpressure semantics
/// match the old inline send. The slack on top covers the non-Input events
/// interleaved into the same channel (one `EndOfRound` per round).
const SEND_PUMP_DEPTH: usize = tango_pvp::battle::MAX_QUEUE_LENGTH + 8;

/// Sender's short wall-clock stamp (ms, wrapping into a u16) for a ping. The
/// pong echoes it; the round-trip delta is the latency sample.
fn ping_timestamp() -> u16 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u16
}

/// Shared per-match stream state: the outbound seq/redundancy window
/// ([`stream::OutStream`]) and the inbound reorder/ack machinery
/// ([`stream::InStream`]). Locked briefly — no awaits held — from the send
/// pump, the receive loop, and the session's in-band EndOfMatch fire.
struct InMatchState {
    out: stream::OutStream,
    inn: stream::InStream,
}

/// Send handle for the unreliable in-match channel (WebRTC stream-1 data
/// channel, or the QUIC datagram channel on the direct path). Shared by the
/// per-frame input pump, the receive loop's ping/pong + ack replies, and the
/// session's in-band `EndOfMatch`. Carries [`wire`] frames + ping/pong over
/// [`Sender::send_raw`].
#[derive(Clone)]
pub struct InMatchTx {
    state: std::sync::Arc<std::sync::Mutex<InMatchState>>,
    sink: std::sync::Arc<tokio::sync::Mutex<Sender>>,
}

impl InMatchTx {
    pub fn new(sink: std::sync::Arc<tokio::sync::Mutex<Sender>>) -> Self {
        Self {
            state: std::sync::Arc::new(std::sync::Mutex::new(InMatchState {
                out: stream::OutStream::new(),
                inn: stream::InStream::new(),
            })),
            sink,
        }
    }

    /// Push an element, snapshot the current redundancy window + block-ack into
    /// one frame, and ship it. The state lock is dropped before the await.
    async fn send_frame_with(&self, push: impl FnOnce(&mut stream::OutStream)) -> std::io::Result<()> {
        let frame = {
            let mut st = self.state.lock().unwrap();
            push(&mut st.out);
            let (base, fa, entries) = st.out.window().expect("window is non-empty after a push");
            let ack = st.inn.block_ack();
            wire::Wire::Frame(wire::Frame {
                base,
                frame_advantage: Some(fa),
                entries,
                ack: Some(ack),
            })
        };
        self.sink.lock().await.send_raw(&frame.encode()).await
    }

    pub async fn send_input(&self, joyflags: u16, frame_advantage: i16) -> std::io::Result<()> {
        self.send_frame_with(move |out| {
            out.push_input(joyflags, frame_advantage);
        })
        .await
    }

    pub async fn send_end_of_round(&self) -> std::io::Result<()> {
        self.send_frame_with(move |out| {
            out.push_marker(wire::Marker::EndOfRound);
        })
        .await
    }

    /// In-band match-end: rides the same ordered seq stream as inputs, so the
    /// peer sees it exactly once and only after every preceding input.
    pub async fn send_end_of_match(&self) -> std::io::Result<()> {
        self.send_frame_with(move |out| {
            out.push_marker(wire::Marker::EndOfMatch);
        })
        .await
    }

    async fn send_ping(&self, ts: u16) -> std::io::Result<()> {
        self.sink.lock().await.send_raw(&wire::Wire::Ping(ts).encode()).await
    }

    async fn send_pong(&self, ts: u16) -> std::io::Result<()> {
        self.sink.lock().await.send_raw(&wire::Wire::Pong(ts).encode()).await
    }

    /// Apply an incoming frame: feed its block-ack to the out-stream and its
    /// entries to the in-stream. Returns the newly-contiguous elements (seq
    /// order) plus the freshest frame-advantage. `Err` => a gap blew past the
    /// rollback horizon and the match must tear down.
    fn recv(&self, frame: &wire::Frame) -> Result<(Vec<wire::Element>, Option<i16>), stream::HorizonExceeded> {
        let mut st = self.state.lock().unwrap();
        if let Some(ack) = frame.ack {
            st.out.apply_ack(ack);
        }
        let delivered = st.inn.accept(frame)?;
        let fa = st.inn.latest_advantage();
        Ok((delivered, fa))
    }
}

/// `tango_pvp::net::Sender` adapter — pushes each per-frame `Event` through a
/// bounded pump ([`SEND_PUMP_DEPTH`]) that ships it as a [`wire`] frame over
/// the unreliable in-match channel. The pump keeps the emulator thread off the
/// shared sink mutex (contended by the receive loop's ping/pong + ack replies)
/// and off the await, and preserves Input/EndOfRound ordering into the
/// out-stream's seq space.
pub struct PvpSender {
    tx: tokio::sync::mpsc::Sender<tango_pvp::net::Event>,
}

impl PvpSender {
    pub fn new(im: InMatchTx) -> Self {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<tango_pvp::net::Event>(SEND_PUMP_DEPTH);
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                let result = match event {
                    tango_pvp::net::Event::Input(input) => im.send_input(input.joyflags, input.frame_advantage).await,
                    tango_pvp::net::Event::EndOfRound => im.send_end_of_round().await,
                };
                if let Err(e) = result {
                    // Dropping rx closes the channel; the next trap-side send
                    // sees BrokenPipe and cancels the match, same as an inline
                    // send failure would have.
                    log::error!("pvp send pump: {e}");
                    break;
                }
            }
        });
        Self { tx }
    }
}

#[async_trait::async_trait]
impl tango_pvp::net::Sender for PvpSender {
    async fn send(&mut self, event: &tango_pvp::net::Event) -> std::io::Result<()> {
        self.tx
            .send(event.clone())
            .await
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pvp send pump terminated"))
    }
}

/// `tango_pvp::net::Receiver` adapter — reads [`wire`] frames off the
/// unreliable in-match channel, feeds them through the shared [`InMatchTx`]
/// reassembly, and yields the resulting `Event`s in strict seq order. Ping is
/// answered with Pong; Pong marks a latency sample; an in-band `EndOfMatch`
/// marker raises `remote_ended`. One frame can deliver several elements, so
/// surplus events buffer in `pending` and drain before the next read. The ping
/// timer keeps the round-trip clock ticking on the live match's actual
/// (unreliable) path.
pub struct PvpReceiver {
    receiver: Receiver,
    im: InMatchTx,
    /// `None` once the remote drops — the session swaps the counter out so the
    /// UI can tell "no live link" from "0 ms ping on LAN". While the link is up
    /// it's `Some` and ping samples land here.
    latency_counter: std::sync::Arc<tokio::sync::Mutex<Option<LatencyCounter>>>,
    ping_timer: tokio::time::Interval,
    /// Flipped `true` the first time an in-band `EndOfMatch` marker is
    /// delivered. `PvpSession::is_ended` reads this to know the remote reached
    /// its match_end_ret hook and the connection is safe to tear down.
    remote_ended: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Session subscription wake, pinged after `remote_ended` flips so
    /// `is_ended` is re-checked without waiting on the next vblank.
    end_of_match_notify: std::sync::Arc<tokio::sync::Notify>,
    /// Elements made contiguous by the last frame but not yet yielded.
    pending: std::collections::VecDeque<tango_pvp::net::Event>,
}

impl PvpReceiver {
    pub fn new(
        receiver: Receiver,
        im: InMatchTx,
        latency_counter: std::sync::Arc<tokio::sync::Mutex<Option<LatencyCounter>>>,
        remote_ended: std::sync::Arc<std::sync::atomic::AtomicBool>,
        end_of_match_notify: std::sync::Arc<tokio::sync::Notify>,
    ) -> Self {
        Self {
            receiver,
            im,
            latency_counter,
            ping_timer: tokio::time::interval(PING_INTERVAL),
            remote_ended,
            end_of_match_notify,
            pending: std::collections::VecDeque::new(),
        }
    }
}

#[async_trait::async_trait]
impl tango_pvp::net::Receiver for PvpReceiver {
    async fn receive(&mut self) -> std::io::Result<tango_pvp::net::Event> {
        loop {
            if let Some(event) = self.pending.pop_front() {
                return Ok(event);
            }
            tokio::select! {
                _ = self.ping_timer.tick() => {
                    self.im.send_ping(ping_timestamp()).await?;
                }
                msg = self.receiver.recv_raw() => {
                    match wire::Wire::decode(&msg?)? {
                        wire::Wire::Ping(ts) => {
                            self.im.send_pong(ts).await?;
                        }
                        wire::Wire::Pong(ts) => {
                            let dt = ping_timestamp().wrapping_sub(ts);
                            if let Some(c) = self.latency_counter.lock().await.as_mut() {
                                c.mark(std::time::Duration::from_millis(dt as u64));
                            }
                        }
                        wire::Wire::Frame(frame) => {
                            let (delivered, fa) = self.im.recv(&frame).map_err(|_| {
                                std::io::Error::new(
                                    std::io::ErrorKind::Other,
                                    "remote overflowed our input buffer",
                                )
                            })?;
                            let frame_advantage = fa.unwrap_or(0);
                            for element in delivered {
                                match element {
                                    wire::Element::Input(joyflags) => {
                                        self.pending.push_back(tango_pvp::net::Event::Input(
                                            tango_pvp::net::Input { joyflags, frame_advantage },
                                        ));
                                    }
                                    wire::Element::Marker(wire::Marker::EndOfRound) => {
                                        self.pending.push_back(tango_pvp::net::Event::EndOfRound);
                                    }
                                    wire::Element::Marker(wire::Marker::EndOfMatch) => {
                                        self.remote_ended.store(true, std::sync::atomic::Ordering::Release);
                                        self.end_of_match_notify.notify_one();
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
