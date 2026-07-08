//! WebRTC data channels: their specs, and the adapters that turn one into a
//! transport-agnostic `Sender` / `Receiver` pair — [`control_pair`] for the
//! control plane's typed-`Packet` transport, [`data_pair`] for the data plane's
//! raw-bytes one.
//!
//! Single source of truth for the two channels every netplay transport brings
//! up:
//!
//! * the **reliable, ordered** control/lobby channel (`"tango"`) carrying the
//!   [`super::control::protocol`] `Packet` stream, and
//! * the **unreliable, unordered** in-match channel (`"tango-match"`) carrying
//!   the live match's [`super::data`] `wire` datagrams.
//!
//! Both the matchmaking path (via [`tango_signaling`], which takes these specs
//! and declares every channel up front) and the signaling-free direct path
//! ([`super::direct_rtc`]) create exactly these. The labels and reliability
//! live here and nowhere else so the two peers can't drift out of agreement —
//! channels are negotiated in-band (DCEP) and matched up by label, so both
//! sides must declare the same set.

use super::{control, data, PacketSink, PacketStream};
use tango_rtc::{ChannelConfig, PeerConnection};

/// The two netplay channels (reliable control + unreliable in-match) plus the
/// peer connection that owns them, as one bundle. Produced by every transport's
/// bring-up *and* rebuild: the signaling-free [`super::direct_rtc`]
/// `host`/`connect`, and the matchmaking / reconnect paths that split the
/// signaling client's channel `Vec` into this shape. The caller keeps
/// `peer_conn` alive for the channels' lifetime.
pub struct Channels {
    /// Reliable, ordered — the control/lobby `Packet` protocol.
    pub control: (control::Sender, control::Receiver),
    /// Unreliable, unordered — the in-match `data::wire` datagrams.
    pub in_match: (data::Sender, data::Receiver),
    pub peer_conn: PeerConnection,
    /// This connection's two DTLS certificate fingerprints (raw SHA-256 bytes),
    /// parsed from the offer/answer SDP, used to seed the matchmaking reconnect
    /// `session_id` (see `netplay::derive_reconnect_session_id`). Empty on a
    /// transport that doesn't surface them — the direct path exchanges no SDP
    /// and runs with fingerprint verification off.
    pub local_dtls_fingerprint: Vec<u8>,
    pub peer_dtls_fingerprint: Vec<u8>,
}

impl Channels {
    /// Build the bundle from a freshly-connected matchmaking session: split the
    /// signaling client's channel `Vec` into [control, in-match] (the spec order
    /// we always pass — see [`control_channel`] / [`in_match_channel`]), pair
    /// each, and carry the connection's DTLS fingerprints through. The initial
    /// connect and a mid-match reconnect both funnel through here, so they bundle
    /// a matchmaking connection identically.
    pub fn from_signaling(connected: tango_signaling::Connected) -> std::io::Result<Self> {
        let tango_signaling::Connected {
            channels: dcs,
            peer_conn,
            local_dtls_fingerprint,
            peer_dtls_fingerprint,
        } = connected;
        let [control_dc, in_match_dc] = <[_; 2]>::try_from(dcs)
            .map_err(|dcs: Vec<_>| std::io::Error::other(format!("expected 2 data channels, got {}", dcs.len())))?;
        Ok(Self {
            control: control_pair(control_dc),
            in_match: data_pair(in_match_dc),
            peer_conn,
            local_dtls_fingerprint,
            peer_dtls_fingerprint,
        })
    }
}

impl std::fmt::Debug for Channels {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Channels { .. }")
    }
}

/// Spec for the reliable control channel. Every transport passes this (with
/// [`in_match_channel`], in this order) to its `PeerConnection` constructor,
/// which declares them together before any exchange.
pub fn control_channel() -> ChannelConfig {
    ChannelConfig {
        label: "tango".to_owned(),
        ordered: true,
        reliable: true,
    }
}

/// Spec for the unreliable in-match channel. Mirror of [`control_channel`] —
/// created alongside it rather than added after the connection is up.
pub fn in_match_channel() -> ChannelConfig {
    ChannelConfig {
        label: "tango-match".to_owned(),
        ordered: false,
        reliable: false,
    }
}

// --- DataChannel <-> Sender/Receiver adapter ------------------------------

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

/// Wrap a `DataChannel`'s two halves into the shared [`PacketSink`] /
/// [`PacketStream`] byte-pipe both planes' transports build on.
fn split(dc: tango_rtc::DataChannel) -> (Box<dyn PacketSink>, Box<dyn PacketStream>) {
    let (dc_tx, dc_rx) = dc.split();
    (
        Box::new(DataChannelSink { inner: dc_tx }),
        Box::new(DataChannelStream { inner: dc_rx }),
    )
}

/// Pair a `DataChannel` into the control plane's typed-`Packet`
/// [`control::Sender`] / [`control::Receiver`] — the reliable control channel.
/// The unreliable in-match channel pairs via [`data_pair`] instead. The peer
/// connection that owns the channel must be kept alive separately (see
/// `netplay::NegotiationOutput`).
pub fn control_pair(dc: tango_rtc::DataChannel) -> (control::Sender, control::Receiver) {
    let (sink, stream) = split(dc);
    (control::Sender::new(sink), control::Receiver::new(stream))
}

/// Pair a `DataChannel` into the data plane's raw-bytes [`data::Sender`] /
/// [`data::Receiver`] — the in-match counterpart to [`control_pair`].
pub fn data_pair(dc: tango_rtc::DataChannel) -> (data::Sender, data::Receiver) {
    let (sink, stream) = split(dc);
    (data::Sender::new(sink), data::Receiver::new(stream))
}
