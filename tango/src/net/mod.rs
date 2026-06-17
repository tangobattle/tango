//! Per-peer netplay transport: a Sender + Receiver pair backed by a
//! pluggable message-oriented transport (`PacketSink` / `PacketStream`),
//! plus `negotiate()` (exchange Hellos and check the protocol versions
//! agree).
//!
//! The WebRTC DataChannel impl lives in [`datachannel`]; the
//! transport-agnostic framing and packet helpers live here.
//!
//! The reliable control/lobby `Packet` protocol is [`protocol`]; the live
//! match's loss-tolerant per-frame wire protocol — the `InMatchTx` /
//! `PvpSender` / `PvpReceiver` adapters used by the battle loop — is [`data`],
//! which runs over a separate **unreliable** in-match data channel.

pub mod channel;
pub mod data;
pub mod datachannel;
pub mod direct_rtc;
pub mod protocol;

pub use data::{InMatchTx, PvpReceiver, PvpSender};

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

    /// Ship a pre-serialized payload straight to the transport. The control
    /// plane's typed `Packet` helpers below frame through here; the data
    /// plane's `data::wire` framing writes its in-match datagrams through here
    /// too (over the unreliable channel).
    pub async fn send_raw(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.sink.send(bytes).await
    }

    pub async fn send_packet(&mut self, p: &protocol::Packet) -> std::io::Result<()> {
        self.send_raw(p.serialize().unwrap().as_slice()).await
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

    // EndOfRound / EndOfMatch are no longer reliable-channel packets — they
    // ride in-band as `data::wire` markers on the unreliable in-match channel
    // (see [`data::InMatchTx`]), so their old send helpers are gone.
}

pub struct Receiver {
    stream: Box<dyn PacketStream>,
}

impl Receiver {
    pub fn new(stream: Box<dyn PacketStream>) -> Self {
        Self { stream }
    }

    /// Read one raw transport message. The receive counterpart to
    /// [`Sender::send_raw`] — the data plane decodes its `wire` frames off
    /// this, and the in-match disconnect watch drains it for the channel's EOF.
    pub async fn recv_raw(&mut self) -> std::io::Result<Vec<u8>> {
        self.stream.recv().await
    }

    pub async fn receive(&mut self) -> std::io::Result<protocol::Packet> {
        let bytes = self.recv_raw().await?;
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
