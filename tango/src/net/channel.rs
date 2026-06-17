//! WebRTC data channels: their specs, and the adapter that turns one into a
//! transport-agnostic [`Sender`] / [`Receiver`] pair.
//!
//! Single source of truth for the two channels every netplay transport brings
//! up:
//!
//! * the **reliable, ordered** control/lobby channel (stream 0) carrying the
//!   [`super::control::protocol`] `Packet` stream, and
//! * the **unreliable, unordered** in-match channel (stream 1) carrying the
//!   live match's [`super::data`] `wire` datagrams.
//!
//! Both the matchmaking path (via [`tango_signaling`], which takes these specs
//! and creates the channels up front) and the signaling-free direct path
//! ([`super::direct_rtc`]) create exactly these. The labels, stream ids, and
//! reliability live here and nowhere else so the two peers can't drift out of
//! agreement.
//!
//! All channels are *negotiated* (pre-agreed stream ids, no in-band DCEP), so
//! both sides just create them with matching ids — no DCEP open handshake.

use super::{PacketSink, PacketStream, Receiver, Sender};
use datachannel_wrapper::{DataChannelInit, Reliability};

/// Label + init for the reliable control channel, as a
/// [`tango_signaling::ChannelSpec`] (the matchmaking path passes every channel's
/// spec to `connect`, which creates them all up front and clones each per
/// transparent reconnect).
pub fn control_channel() -> (&'static str, DataChannelInit) {
    (
        "tango",
        DataChannelInit::default().negotiated().manual_stream().stream(0),
    )
}

/// Label + init for the unreliable in-match channel, as a
/// [`tango_signaling::ChannelSpec`]. Mirror of [`control_channel`] — the
/// matchmaking path creates this alongside the control channel rather than
/// adding it after the connection is up.
pub fn in_match_channel() -> (&'static str, DataChannelInit) {
    (
        "tango-match",
        DataChannelInit::default()
            .reliability(Reliability {
                unordered: true,
                unreliable: true,
                max_packet_life_time: 0,
                max_retransmits: 0,
            })
            .negotiated()
            .manual_stream()
            .stream(1),
    )
}

// --- DataChannel <-> Sender/Receiver adapter ------------------------------

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

struct DataChannelStream {
    inner: datachannel_wrapper::DataChannelReceiver,
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
/// pair. The peer connection that owns the channel must be kept alive
/// separately (see `netplay::NegotiationOutput`).
pub fn pair(dc: datachannel_wrapper::DataChannel) -> (Sender, Receiver) {
    let (dc_tx, dc_rx) = dc.split();
    let sender = Sender::new(Box::new(DataChannelSink { inner: dc_tx }));
    let receiver = Receiver::new(Box::new(DataChannelStream { inner: dc_rx }));
    (sender, receiver)
}
