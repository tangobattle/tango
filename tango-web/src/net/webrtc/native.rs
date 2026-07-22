//! libdatachannel peer transport via `datachannel-wrapper` — the same
//! stack the desktop client's signaling driver uses, adapted to this
//! crate's browser-shaped API. Auto-negotiation is off: `create_offer`
//! installs the local offer explicitly (trickle — the SDP ships before
//! gathering finishes, candidates follow via events), and the polite
//! side's `rollback_and_accept_offer` uses libdatachannel's real
//! `Rollback` support, mirroring the browser flow one-to-one.
//!
//! libdatachannel's callbacks fire on its own network threads; the
//! wrapper forwards them onto tokio channels. Small pump tasks on the
//! net runtime ([`crate::net::rt`]) adapt those to this API's shape:
//! sync clone-able sends (queued through an unbounded channel), async
//! receives (a futures channel the main-thread executor can poll), and
//! the connection-event stream.

use std::sync::{Arc, Mutex};

use datachannel_wrapper::{
    ConnectionState, DataChannelInit, IceCandidate, PeerConnectionEvent, Reliability, RtcConfig, SdpType,
    SessionDescription, TransportPolicy,
};
use futures::channel::{mpsc, oneshot};
use futures::StreamExt;

use tango_net_protocol::channel_spec;

use super::{parse_dtls_fingerprint, PeerEvent};
use crate::net::rt;

/// One live peer connection. Dropping it tears the transport down.
pub struct PeerConnection {
    pc: Arc<Mutex<datachannel_wrapper::PeerConnection>>,
    /// Local descriptions published by the `on_local_description`
    /// callback, latest wins. `create_offer`/`rollback_and_accept_offer`
    /// read it after their `set_local_description` — the callback has
    /// already fired synchronously by then, but `local_description()`
    /// on the connection is the fallback either way.
    local_desc: tokio::sync::watch::Receiver<Option<SessionDescription>>,
}

/// The sending half of a datachannel. `send` is synchronous — a pump
/// task drains the queue into libdatachannel (whose sends are sync
/// once the channel is open; the pump's first send awaits the open).
#[derive(Clone)]
pub struct ChannelSender {
    tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
}

impl ChannelSender {
    pub fn send(&self, bytes: &[u8]) -> anyhow::Result<()> {
        self.tx
            .send(bytes.to_vec())
            .map_err(|_| anyhow::anyhow!("datachannel send: channel closed"))
    }

    #[allow(dead_code)] // teardown drain (M4)
    pub fn buffered_amount(&self) -> u32 {
        // Not surfaced by datachannel-wrapper; only consulted for
        // best-effort teardown draining.
        0
    }
}

/// The receiving half: `None` once the channel has closed.
pub struct ChannelReceiver {
    rx: mpsc::UnboundedReceiver<Vec<u8>>,
}

impl ChannelReceiver {
    pub async fn receive(&mut self) -> Option<Vec<u8>> {
        self.rx.next().await
    }
}

/// Everything [`new`] hands back for one connection.
pub struct PeerParts {
    pub pc: PeerConnection,
    pub events: mpsc::UnboundedReceiver<PeerEvent>,
    pub control_tx: ChannelSender,
    pub control_rx: ChannelReceiver,
    pub in_match_tx: ChannelSender,
    pub in_match_rx: ChannelReceiver,
    /// Resolves when the control channel opens (the handshake barrier).
    pub control_open: oneshot::Receiver<()>,
}

/// Build the peer connection with tango's two negotiated channels from
/// the server-supplied ICE set. `use_relay`: `Some(true)` forces the
/// relay-only transport policy, `Some(false)` strips TURN servers,
/// `None` takes everything the server offered.
pub fn new(
    ice_servers: &[tango_signaling::proto::signaling::packet::hello::IceServer],
    use_relay: Option<bool>,
) -> anyhow::Result<PeerParts> {
    // libdatachannel ICE URI format, credentials inline — the same
    // massage tango-signaling's desktop client does (including skipping
    // TURN-over-TCP, which libdatachannel doesn't support).
    let mut urls = Vec::new();
    for server in ice_servers {
        for url in &server.urls {
            let Some(colon_idx) = url.find(':') else { continue };
            let proto = &url[..colon_idx];
            let rest = &url[colon_idx + 1..];
            let is_turn = proto == "turn" || proto == "turns";
            if is_turn && use_relay == Some(false) {
                continue;
            }
            if url.ends_with("?transport=tcp") {
                continue;
            }
            if let (Some(username), Some(credential)) = (&server.username, &server.credential) {
                urls.push(format!(
                    "{}:{}:{}@{}",
                    proto,
                    urlencoding::encode(username),
                    urlencoding::encode(credential),
                    rest
                ));
            } else {
                urls.push(format!("{proto}:{rest}"));
            }
        }
    }
    let mut config = RtcConfig::new(&urls);
    if use_relay == Some(true) {
        config.ice_transport_policy = TransportPolicy::Relay;
    }
    // The offer is installed explicitly in `create_offer` — with
    // auto-negotiation on, creating the first channel would race it.
    config.disable_auto_negotiation = true;

    let (mut pc, mut wrapper_events) = datachannel_wrapper::PeerConnection::new(config)?;

    // Both channels are negotiated on fixed stream ids, so both sides
    // just create them — no in-band open announcement. The stream id is
    // what's load-bearing against the peer; the labels match for
    // sanity.
    let control = pc.create_data_channel(
        channel_spec::CONTROL_LABEL,
        DataChannelInit::default()
            .negotiated()
            .manual_stream()
            .stream(channel_spec::CONTROL_STREAM_ID),
    )?;
    let in_match = pc.create_data_channel(
        channel_spec::IN_MATCH_LABEL,
        DataChannelInit::default()
            .reliability(Reliability {
                unordered: true,
                unreliable: true,
                max_packet_life_time: 0,
                max_retransmits: 0,
            })
            .negotiated()
            .manual_stream()
            .stream(channel_spec::IN_MATCH_STREAM_ID),
    )?;

    let (control_tx, control_rx) = wire_channel(control);
    let (in_match_tx, in_match_rx) = wire_channel(in_match);

    // The event pump: wrapper events (tokio, fed from libdatachannel's
    // threads) → this API's futures channel + the local-description
    // watch + the control-open barrier.
    let (event_tx, events) = mpsc::unbounded::<PeerEvent>();
    let (desc_tx, local_desc) = tokio::sync::watch::channel::<Option<SessionDescription>>(None);
    let (open_tx, control_open) = oneshot::channel::<()>();
    rt::handle().spawn(async move {
        let mut open_tx = Some(open_tx);
        while let Some(ev) = wrapper_events.recv().await {
            match ev {
                PeerConnectionEvent::SessionDescription(desc) => {
                    let _ = desc_tx.send(Some(desc));
                }
                PeerConnectionEvent::IceCandidate(c) => {
                    if !c.candidate.is_empty() {
                        let _ = event_tx.unbounded_send(PeerEvent::Candidate(c.candidate));
                    }
                }
                PeerConnectionEvent::ConnectionStateChange(state) => match state {
                    ConnectionState::Connected => {
                        // Negotiated channels are usable once the
                        // association is up; the senders' own open
                        // barriers cover the rest.
                        if let Some(tx) = open_tx.take() {
                            let _ = tx.send(());
                        }
                        let _ = event_tx.unbounded_send(PeerEvent::Connected);
                    }
                    ConnectionState::Failed | ConnectionState::Closed => {
                        let _ = event_tx.unbounded_send(PeerEvent::Failed);
                    }
                    // "disconnected" can self-heal; let ICE keep trying.
                    _ => {}
                },
                PeerConnectionEvent::GatheringStateChange(_) => {}
            }
        }
        event_tx.close_channel();
    });

    Ok(PeerParts {
        pc: PeerConnection {
            pc: Arc::new(Mutex::new(pc)),
            local_desc,
        },
        events,
        control_tx,
        control_rx,
        in_match_tx,
        in_match_rx,
        control_open,
    })
}

/// Adapt one wrapper `DataChannel` to the sync-send / async-recv
/// facade: a send pump draining an unbounded queue (its `send` awaits
/// the channel open, so pre-open sends just wait there), and a recv
/// pump forwarding messages until EOF.
fn wire_channel(dc: datachannel_wrapper::DataChannel) -> (ChannelSender, ChannelReceiver) {
    let (dc_tx, mut dc_rx) = dc.split();
    let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    rt::handle().spawn(async move {
        let mut dc_tx = dc_tx;
        while let Some(msg) = out_rx.recv().await {
            if let Err(e) = dc_tx.send(&msg).await {
                log::warn!("datachannel send: {e}");
                break;
            }
        }
    });
    let (in_tx, in_rx) = mpsc::unbounded::<Vec<u8>>();
    rt::handle().spawn(async move {
        while let Some(msg) = dc_rx.receive().await {
            if in_tx.unbounded_send(msg).is_err() {
                break;
            }
        }
        in_tx.close_channel();
    });
    (ChannelSender { tx: out_tx }, ChannelReceiver { rx: in_rx })
}

impl PeerConnection {
    /// Create and install the local offer; returns its SDP for the
    /// signaling relay (candidates trickle separately).
    pub async fn create_offer(&self) -> anyhow::Result<String> {
        self.pc
            .lock()
            .unwrap()
            .set_local_description(SdpType::Offer, None)?;
        self.local_sdp()
    }

    /// Perfect negotiation, polite side: roll our un-answered offer
    /// back, install the peer's offer, and produce our installed
    /// answer's SDP.
    pub async fn rollback_and_accept_offer(&self, sdp: &str) -> anyhow::Result<String> {
        {
            let mut pc = self.pc.lock().unwrap();
            pc.set_local_description(SdpType::Rollback, None)?;
            pc.set_remote_description(SessionDescription {
                sdp_type: SdpType::Offer,
                sdp: sdp.to_owned(),
            })?;
            pc.set_local_description(SdpType::Answer, None)?;
        }
        self.local_sdp()
    }

    pub async fn accept_answer(&self, sdp: &str) -> anyhow::Result<()> {
        self.pc.lock().unwrap().set_remote_description(SessionDescription {
            sdp_type: SdpType::Answer,
            sdp: sdp.to_owned(),
        })?;
        Ok(())
    }

    fn local_sdp(&self) -> anyhow::Result<String> {
        self.pc
            .lock()
            .unwrap()
            .local_description()
            .or_else(|| self.local_desc.borrow().clone())
            .map(|d| d.sdp)
            .ok_or_else(|| anyhow::anyhow!("local description missing"))
    }

    /// This side's DTLS certificate fingerprint (raw SHA-256 bytes),
    /// parsed from the local SDP. Empty if unparsable — callers must
    /// tolerate that (the reconnect id has a seed-only fallback).
    pub fn local_dtls_fingerprint(&self) -> Vec<u8> {
        self.pc
            .lock()
            .unwrap()
            .local_description()
            .and_then(|d| parse_dtls_fingerprint(&d.sdp))
            .unwrap_or_default()
    }

    /// The peer's DTLS certificate fingerprint from the remote SDP.
    pub fn peer_dtls_fingerprint(&self) -> Vec<u8> {
        self.pc
            .lock()
            .unwrap()
            .remote_description()
            .and_then(|d| parse_dtls_fingerprint(&d.sdp))
            .unwrap_or_default()
    }

    pub async fn add_remote_candidate(&self, candidate: &str) -> anyhow::Result<()> {
        self.pc.lock().unwrap().add_remote_candidate(IceCandidate {
            candidate: candidate.to_owned(),
        })?;
        Ok(())
    }

    #[allow(dead_code)] // session teardown (M4)
    pub fn close(&self) {
        // Dropping the wrapper connection tears the transport down;
        // there's no separate close. The struct owner dropping us is
        // the actual teardown path.
    }
}
