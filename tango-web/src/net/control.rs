//! Control plane, browser flavor: typed `Packet`s framed one per SCTP
//! message over the reliable channel, plus the version [`negotiate`]
//! handshake — the same semantics as the desktop's `net::control`
//! (Hello first, exact version match), synchronous sends (the browser
//! buffers) and async receives.

use tango_net_protocol::control as protocol;

use super::webrtc::{ChannelReceiver, ChannelSender};

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
pub async fn negotiate(tx: &Sender, rx: &mut Receiver) -> Result<(), NegotiationError> {
    tx.send_hello().map_err(NegotiationError::Other)?;
    let hello = match rx.receive().await.map_err(|_| NegotiationError::ExpectedHello)? {
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

/// Send half: typed packets over the reliable channel. Synchronous —
/// the browser's own buffer absorbs bursts; the reveal chunks are the
/// biggest thing that crosses (32 KiB each, well under the buffer).
#[derive(Clone)]
pub struct Sender {
    tx: ChannelSender,
}

impl Sender {
    pub fn new(tx: ChannelSender) -> Self {
        Self { tx }
    }

    pub fn send_packet(&self, p: &protocol::Packet) -> anyhow::Result<()> {
        self.tx.send(p.serialize().unwrap().as_slice())
    }

    pub fn send_hello(&self) -> anyhow::Result<()> {
        self.send_packet(&protocol::Packet::Hello(protocol::Hello {
            protocol_version: protocol::VERSION,
        }))
    }

    pub fn send_ping(&self, ts: u16) -> anyhow::Result<()> {
        self.send_packet(&protocol::Packet::Ping(protocol::Ping { ts }))
    }

    pub fn send_pong(&self, ts: u16) -> anyhow::Result<()> {
        self.send_packet(&protocol::Packet::Pong(protocol::Pong { ts }))
    }

    pub fn send_settings(&self, settings: protocol::Settings) -> anyhow::Result<()> {
        self.send_packet(&protocol::Packet::Settings(settings))
    }

    pub fn send_commit(&self, commitment: [u8; 16]) -> anyhow::Result<()> {
        self.send_packet(&protocol::Packet::Commit(protocol::Commit { commitment }))
    }

    pub fn send_uncommit(&self) -> anyhow::Result<()> {
        self.send_packet(&protocol::Packet::Uncommit(protocol::Uncommit {}))
    }

    pub fn send_chunk_start(&self, len: u64) -> anyhow::Result<()> {
        self.send_packet(&protocol::Packet::ChunkStart(protocol::ChunkStart { len }))
    }

    pub fn send_chunk(&self, chunk: Vec<u8>) -> anyhow::Result<()> {
        self.send_packet(&protocol::Packet::Chunk(protocol::Chunk { chunk }))
    }

    pub fn send_start_match(&self) -> anyhow::Result<()> {
        self.send_packet(&protocol::Packet::StartMatch(protocol::StartMatch {}))
    }

    #[allow(dead_code)] // deliberate-quit announce (M4)
    pub fn send_goodbye(&self) -> anyhow::Result<()> {
        self.send_packet(&protocol::Packet::Goodbye(protocol::Goodbye {}))
    }
}

/// Receive half: the next typed packet off the reliable channel.
/// Errors on close (`UnexpectedEof`-like) or an undecodable frame.
pub struct Receiver {
    rx: ChannelReceiver,
}

impl Receiver {
    pub fn new(rx: ChannelReceiver) -> Self {
        Self { rx }
    }

    pub async fn receive(&mut self) -> anyhow::Result<protocol::Packet> {
        let raw = self
            .rx
            .receive()
            .await
            .ok_or_else(|| anyhow::anyhow!("control channel closed"))?;
        protocol::Packet::deserialize(&raw).map_err(|e| anyhow::anyhow!("bad control packet: {e}"))
    }
}
