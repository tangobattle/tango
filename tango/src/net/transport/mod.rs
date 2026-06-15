//! Transport layer: the byte-pipe abstraction ([`PacketSink`] /
//! [`PacketStream`]) and its concrete implementations (WebRTC data channel for
//! the matchmaking path, QUIC for the direct link-code path), wrapped as a
//! [`Sender`] / [`Receiver`] pair that moves raw framed messages.
//!
//! This layer knows nothing about packet contents. The control plane
//! ([`super::control`]) and the data plane ([`super::data`]) layer their own
//! framing on top via [`Sender::send_raw`] / [`Receiver::recv_raw`].

pub mod datachannel;
pub mod quic;

/// Default UDP port for the direct link-code transport (commands `/host`
/// and `/connect`). The host's QUIC connection multiplexes both netplay
/// channels over this one port, so it's the only thing to port-forward.
/// `24680` reads as a memorable even-step sequence and steers clear of every
/// well-known service in the ephemeral range — easy to type, easy to recite
/// over voice chat, unlikely to clash with anything already listening locally.
pub const DEFAULT_LOCAL_PORT: u16 = 24680;

/// One half of a peer connection's send side. Carries discrete,
/// reliable, in-order byte messages — same contract as a WebRTC
/// DataChannel configured `unordered: false, unreliable: false`. A
/// byte-stream-backed impl (a QUIC stream) must add its own
/// length-prefix framing so each `send` round-trips as exactly one
/// `recv` on the peer. (The unreliable in-match channels relax the
/// ordering/reliability half of this; the data plane is built to
/// tolerate it.)
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

/// The send half of a transport. A bare byte pipe — the typed `Packet`
/// helpers live in [`super::control`], layered on [`send_raw`](Self::send_raw).
pub struct Sender {
    sink: Box<dyn PacketSink>,
}

impl Sender {
    pub fn new(sink: Box<dyn PacketSink>) -> Self {
        Self { sink }
    }

    /// Ship a pre-serialized payload straight to the transport. Both the
    /// control plane's `Packet` framing and the data plane's `wire` framing
    /// write their bytes through here.
    pub async fn send_raw(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.sink.send(bytes).await
    }
}

/// The receive half of a transport. Counterpart to [`Sender`].
pub struct Receiver {
    stream: Box<dyn PacketStream>,
}

impl Receiver {
    pub fn new(stream: Box<dyn PacketStream>) -> Self {
        Self { stream }
    }

    /// Read one raw transport message. The receive counterpart to
    /// [`Sender::send_raw`]; a clean close surfaces as `UnexpectedEof`.
    pub async fn recv_raw(&mut self) -> std::io::Result<Vec<u8>> {
        self.stream.recv().await
    }
}
