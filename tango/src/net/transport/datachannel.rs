//! WebRTC `DataChannel` adapter for the [`super::PacketSink`] /
//! [`super::PacketStream`] traits. Use [`pair`] to build a Sender +
//! Receiver pair from a freshly-opened DataChannel.

use super::{PacketSink, PacketStream, Receiver, Sender};

struct DataChannelSink {
    inner: datachannel_wrapper::DataChannelSender,
}

#[async_trait::async_trait]
impl PacketSink for DataChannelSink {
    async fn send(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.inner.send(bytes).await?;
        Ok(())
    }
}

/// Largest data-channel message we'll accept, on either channel. The lobby's
/// biggest packet is a save-state `Chunk` (chunked under this), and an
/// in-match `wire` frame is tiny; anything larger is malformed or hostile, so
/// reject it rather than hand a peer-sized buffer up the stack. Matches the
/// reliable transport's framing cap (`tcp::MAX_FRAME_BYTES`).
const MAX_MESSAGE_BYTES: usize = 64 * 1024;

struct DataChannelStream {
    inner: datachannel_wrapper::DataChannelReceiver,
}

#[async_trait::async_trait]
impl PacketStream for DataChannelStream {
    async fn recv(&mut self) -> std::io::Result<Vec<u8>> {
        let msg = self
            .inner
            .receive()
            .await
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "stream is empty"))?;
        if msg.len() > MAX_MESSAGE_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("oversized message: {} bytes", msg.len()),
            ));
        }
        Ok(msg)
    }
}

/// Split a `DataChannel` into a transport-agnostic Sender + Receiver
/// pair. The peer connection that owns the channel must be kept alive
/// separately (see `netplay::NegotiationOutput`).
pub fn pair(dc: datachannel_wrapper::DataChannel) -> (Sender, Receiver) {
    let (dc_tx, dc_rx) = dc.split();
    let sender = Sender::new(Box::new(DataChannelSink { inner: dc_tx }));
    let receiver = Receiver::new(Box::new(DataChannelStream { inner: dc_rx }));
    (sender, receiver)
}
