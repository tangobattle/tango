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
pub mod direct_rtc;
pub mod protocol;

/// Default UDP port for the signaling-free direct local-play transport
/// (link-code commands `/host` and `/connect`; see
/// [`direct_rtc`]). `24680` reads as a memorable even-step sequence and
/// steers clear of every well-known service in the ephemeral range —
/// easy to type, easy to recite over voice chat, unlikely to clash with
/// anything already listening locally.
pub const DEFAULT_LOCAL_PORT: u16 = 24680;

/// One half of a peer connection's send side. Carries discrete,
/// reliable, in-order byte messages — same contract as a WebRTC
/// DataChannel configured `unordered: false, unreliable: false`. A
/// stream-oriented impl would have to add its own length-prefix
/// framing so each `send` round-trips as exactly one `recv` on the
/// peer; the DataChannel transports preserve message boundaries
/// natively.
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

    pub async fn send_end_of_round(&mut self) -> std::io::Result<()> {
        self.send_packet(&protocol::Packet::EndOfRound(protocol::EndOfRound {}))
            .await
    }

    pub async fn send_end_of_match(&mut self) -> std::io::Result<()> {
        self.send_packet(&protocol::Packet::EndOfMatch(protocol::EndOfMatch {}))
            .await
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

/// `tango_pvp::net::Sender` adapter — forwards inputs as
/// `Packet::Input` over the shared peer-connection Sender.
///
/// Events go through a bounded channel ([`SEND_PUMP_DEPTH`]) drained by a
/// dedicated pump task rather than being shipped inline: `send` is called
/// once per frame from the emulator thread's `main_read_joyflags` trap, and
/// writing the wire there would make the frame wait on the shared sender
/// mutex (contended by the receive task's ping/pong replies) and on the
/// datachannel itself. A single pump preserves Input/EndOfRound ordering.
pub struct PvpSender {
    tx: tokio::sync::mpsc::Sender<tango_pvp::net::Event>,
}

impl PvpSender {
    pub fn new(sender: std::sync::Arc<tokio::sync::Mutex<Sender>>) -> Self {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<tango_pvp::net::Event>(SEND_PUMP_DEPTH);
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                let mut sender = sender.lock().await;
                let result = match event {
                    tango_pvp::net::Event::Input(input) => sender.send_packet(&protocol::Packet::Input(input)).await,
                    tango_pvp::net::Event::EndOfRound => sender.send_end_of_round().await,
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

/// `tango_pvp::net::Receiver` adapter — pulls Input packets,
/// silently round-trips ping/pong, errors on anything else.
/// The ping timer here keeps the round-trip clock ticking
/// during the live match (the lobby loop's interval was for
/// the pre-match phase only).
pub struct PvpReceiver {
    receiver: Receiver,
    sender: std::sync::Arc<tokio::sync::Mutex<Sender>>,
    /// `None` once the remote drops — the session swaps the counter out so the
    /// UI can tell "no live link" from "0 ms ping on LAN". While the link is up
    /// it's `Some` and ping samples land here.
    latency_counter: std::sync::Arc<tokio::sync::Mutex<Option<LatencyCounter>>>,
    ping_timer: tokio::time::Interval,
    /// Flipped to `true` the first time we see an `EndOfMatch`
    /// packet from the remote. `PvpSession::is_ended` reads this
    /// to know the remote has also reached its match_end_ret hook
    /// and the connection is safe to tear down.
    remote_ended: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Session subscription wake. Pinged after `remote_ended` flips
    /// so `is_ended` is re-checked without waiting on the next
    /// vblank — by then the emu thread has already paused.
    end_of_match_notify: std::sync::Arc<tokio::sync::Notify>,
}

impl PvpReceiver {
    pub fn new(
        receiver: Receiver,
        sender: std::sync::Arc<tokio::sync::Mutex<Sender>>,
        latency_counter: std::sync::Arc<tokio::sync::Mutex<Option<LatencyCounter>>>,
        remote_ended: std::sync::Arc<std::sync::atomic::AtomicBool>,
        end_of_match_notify: std::sync::Arc<tokio::sync::Notify>,
    ) -> Self {
        Self {
            receiver,
            sender,
            latency_counter,
            ping_timer: tokio::time::interval(PING_INTERVAL),
            remote_ended,
            end_of_match_notify,
        }
    }
}

#[async_trait::async_trait]
impl tango_pvp::net::Receiver for PvpReceiver {
    async fn receive(&mut self) -> std::io::Result<tango_pvp::net::Event> {
        loop {
            tokio::select! {
                _ = self.ping_timer.tick() => {
                    let now_short = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u16;
                    self.sender
                        .lock()
                        .await
                        .send_ping(now_short)
                        .await?;
                }
                p = self.receiver.receive() => {
                    match p? {
                        protocol::Packet::Ping(ping) => {
                            self.sender.lock().await.send_pong(ping.ts).await?;
                        }
                        protocol::Packet::Pong(pong) => {
                            let now_short = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u16;
                            let dt = now_short.wrapping_sub(pong.ts);

                            if let Some(c) = self.latency_counter.lock().await.as_mut() {
                                c.mark(std::time::Duration::from_millis(dt as u64));
                            }
                        }
                        protocol::Packet::Input(input) => {
                            return Ok(tango_pvp::net::Event::Input(input));
                        }
                        protocol::Packet::EndOfRound(_) => {
                            return Ok(tango_pvp::net::Event::EndOfRound);
                        }
                        protocol::Packet::EndOfMatch(_) => {
                            // Remote reached its match_end_ret hook.
                            // No input to yield; keep looping so
                            // any tail-end Input packets the remote
                            // already queued can still arrive.
                            self.remote_ended.store(true, std::sync::atomic::Ordering::Release);
                            self.end_of_match_notify.notify_one();
                        }
                        p => {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                format!("invalid in-match packet: {p:?}"),
                            ));
                        }
                    }
                }
            }
        }
    }
}
