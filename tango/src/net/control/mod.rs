//! Control plane: the reliable lobby/handshake channel.
//!
//! The [`protocol`] `Packet` wire format (handshake, lobby settings,
//! commitment/chunk exchange, match start), the [`PacketSink`] / [`PacketStream`]
//! byte transport wrapped as a [`Sender`] / [`Receiver`] pair that frames those
//! `Packet`s, and the version [`negotiate`] handshake.
//!
//! Everything here runs over the reliable, ordered channel; the live match's
//! per-frame traffic is the data plane's job ([`super::data`]).
//!
//! `Sender::send_raw` / `Receiver::recv_raw` expose the underlying byte pipe:
//! the data plane frames its own `wire` datagrams through the same pair (over
//! the unreliable channel), and the in-match disconnect watch drains `recv_raw`
//! for the reliable channel's EOF.

pub mod protocol;

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
    // (see [`super::data::InMatchTx`]), so their old send helpers are gone.
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
