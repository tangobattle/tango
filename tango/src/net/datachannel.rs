//! WebRTC `DataChannel` adapter for the [`super::PacketSink`] /
//! [`super::PacketStream`] traits. Use [`pair`] to build a Sender +
//! Receiver pair from a freshly-opened DataChannel.

use super::{PacketSink, PacketStream, Receiver, Sender};

struct DataChannelSink {
    inner: tango_rtc::DataChannelSender,
}

#[async_trait::async_trait]
impl PacketSink for DataChannelSink {
    async fn send(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.inner.send(bytes).await?;
        Ok(())
    }
}

struct DataChannelStream {
    inner: tango_rtc::DataChannelReceiver,
}

#[async_trait::async_trait]
impl PacketStream for DataChannelStream {
    async fn recv(&mut self) -> std::io::Result<Vec<u8>> {
        self.inner
            .receive()
            .await
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "stream is empty"))
    }
}

/// Split a `DataChannel` into a transport-agnostic Sender + Receiver
/// pair. The halves own the underlying connection between them; it
/// hangs up when the last one is dropped.
pub fn pair(dc: tango_rtc::DataChannel) -> (Sender, Receiver) {
    let (dc_tx, dc_rx) = dc.split();
    let sender = Sender::new(Box::new(DataChannelSink { inner: dc_tx }));
    let receiver = Receiver::new(Box::new(DataChannelStream { inner: dc_rx }));
    (sender, receiver)
}
