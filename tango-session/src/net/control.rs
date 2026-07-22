//! Control plane: the reliable lobby/handshake channel.
//!
//! The [`protocol`] `Packet` wire format (handshake, lobby settings,
//! commitment/chunk exchange, match start), the [`Sender`] / [`Receiver`] pair
//! that frames those typed `Packet`s over the shared [`PacketSink`] /
//! [`PacketStream`] byte-pipe, and the version [`negotiate`] handshake.
//!
//! Everything here runs over the reliable, ordered channel; the live match's
//! per-frame traffic — and its own raw-bytes [`Sender`](super::data::Sender) /
//! [`Receiver`](super::data::Receiver) — is the data plane's job
//! ([`super::data`]).

use tango_net_protocol::control as protocol;

use super::{PacketSink, PacketStream};

#[derive(Debug, thiserror::Error)]
pub enum NegotiationError {
    #[error("expected hello")]
    ExpectedHello,
    #[error("remote protocol version too old")]
    RemoteProtocolVersionTooOld,
    #[error("remote protocol version too new")]
    RemoteProtocolVersionTooNew,
    /// The transport failed underneath the handshake.
    #[error(transparent)]
    Other(#[from] std::io::Error),
}

/// Exchange Hello packets with the peer and verify both sides speak
/// the same `protocol::VERSION`. Has to run on both peers before any
/// other packet is sent.
pub async fn negotiate(sender: &mut Sender, receiver: &mut Receiver) -> Result<(), NegotiationError> {
    sender.send_hello().await.map_err(NegotiationError::Other)?;
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

    /// Serialize a typed `Packet` and frame it as one message on the transport.
    /// Every `send_*` helper below funnels through here.
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

    pub async fn send_chunk_start(&mut self, len: u64) -> std::io::Result<()> {
        self.send_packet(&protocol::Packet::ChunkStart(protocol::ChunkStart { len }))
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

    /// Announce a deliberate mid-match quit, just before teardown (see
    /// [`protocol::Packet::Goodbye`]).
    pub async fn send_goodbye(&mut self) -> std::io::Result<()> {
        self.send_packet(&protocol::Packet::Goodbye(protocol::Goodbye {}))
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

    /// Read one message off the transport and decode it as a `Packet`. A
    /// transport-level close surfaces as `io::ErrorKind::UnexpectedEof`;
    /// undecodable bytes as `io::ErrorKind::InvalidData` — callers that tolerate
    /// stray traffic (e.g. the mid-match close watch) discriminate on the
    /// error kind.
    pub async fn receive(&mut self) -> std::io::Result<protocol::Packet> {
        let bytes = self.stream.recv().await?;
        protocol::Packet::deserialize(bytes.as_slice())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}
