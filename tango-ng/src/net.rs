//! Per-peer netplay transport: a Sender + Receiver pair wrapping the
//! WebRTC DataChannel halves, plus `negotiate()` (exchange Hellos and
//! check the protocol versions agree). Ported from `tango/src/net.rs`.
//!
//! `PvpSender` / `PvpReceiver` (the `tango_pvp::net` adapters used by
//! the live battle loop) come in a later netplay round.

pub mod protocol;

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
    let hello = match receiver
        .receive()
        .await
        .map_err(|_| NegotiationError::ExpectedHello)?
    {
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
    dc_tx: datachannel_wrapper::DataChannelSender,
}

impl Sender {
    pub fn new(dc_tx: datachannel_wrapper::DataChannelSender) -> Self {
        Self { dc_tx }
    }

    async fn send_packet(&mut self, p: &protocol::Packet) -> std::io::Result<()> {
        self.dc_tx
            .send(p.serialize().unwrap().as_slice())
            .await?;
        Ok(())
    }

    pub async fn send_hello(&mut self) -> std::io::Result<()> {
        self.send_packet(&protocol::Packet::Hello(protocol::Hello {
            protocol_version: protocol::VERSION,
        }))
        .await
    }

    pub async fn send_ping(&mut self, ts: std::time::SystemTime) -> std::io::Result<()> {
        self.send_packet(&protocol::Packet::Ping(protocol::Ping { ts })).await
    }

    pub async fn send_pong(&mut self, ts: std::time::SystemTime) -> std::io::Result<()> {
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
}

pub struct Receiver {
    dc_rx: datachannel_wrapper::DataChannelReceiver,
}

impl Receiver {
    pub fn new(dc_rx: datachannel_wrapper::DataChannelReceiver) -> Self {
        Self { dc_rx }
    }

    pub async fn receive(&mut self) -> std::io::Result<protocol::Packet> {
        let bytes = self.dc_rx.receive().await.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "stream is empty")
        })?;
        protocol::Packet::deserialize(bytes.as_slice())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}
