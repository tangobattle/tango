//! WebRTC peer transport, browser flavor (after gbaroll's): tango's two
//! negotiated fixed-stream-id datachannels (see
//! [`tango_net_protocol::channel_spec`] — "tango" id 0 reliable+ordered,
//! "tango-match" id 1 unordered, zero retransmits; rennet's redundancy
//! replaces reliability), over `web_sys::RtcPeerConnection`. Sends are
//! synchronous; receives pull from unbounded channels fed by the
//! `onmessage` callbacks; channel opens are explicit awaitable barriers
//! (the web has no blocks-until-open first send).
//!
//! The desktop's libdatachannel driver creates the same two channels
//! with the same ids in negotiated mode, so the SCTP-level identity
//! matches and no in-band DCEP handshake happens on either side.

use std::cell::RefCell;
use std::rc::Rc;

use futures::channel::{mpsc, oneshot};
use futures::StreamExt;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    RtcConfiguration, RtcDataChannel, RtcDataChannelInit, RtcDataChannelType, RtcIceCandidateInit,
    RtcIceTransportPolicy, RtcPeerConnection, RtcPeerConnectionState, RtcSdpType,
    RtcSessionDescriptionInit,
};

use tango_net_protocol::channel_spec;

fn jserr(what: &str, e: JsValue) -> anyhow::Error {
    anyhow::anyhow!("{what}: {e:?}")
}

/// Connection-level events the signaling loop consumes.
pub enum PeerEvent {
    /// A trickled local ICE candidate to relay to the peer.
    Candidate(String),
    /// The connection came up.
    Connected,
    /// The connection failed or closed.
    Failed,
}

/// One live peer connection. Dropping it tears the transport down.
pub struct PeerConnection {
    pc: RtcPeerConnection,
    _closures: Vec<Closure<dyn FnMut(web_sys::Event)>>,
}

/// The sending half of a datachannel. `send` is synchronous — the
/// browser buffers; `buffered_amount` exposes the backlog for teardown
/// draining.
#[derive(Clone)]
pub struct ChannelSender {
    dc: RtcDataChannel,
}

impl ChannelSender {
    pub fn send(&self, bytes: &[u8]) -> anyhow::Result<()> {
        self.dc
            .send_with_u8_array(bytes)
            .map_err(|e| jserr("datachannel send", e))
    }

    #[allow(dead_code)] // teardown drain (M4)
    pub fn buffered_amount(&self) -> u32 {
        self.dc.buffered_amount()
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
    let config = RtcConfiguration::new();
    let servers = js_sys::Array::new();
    for server in ice_servers {
        let urls = js_sys::Array::new();
        for url in &server.urls {
            let is_turn = url.starts_with("turn:") || url.starts_with("turns:");
            if is_turn && use_relay == Some(false) {
                continue;
            }
            urls.push(&JsValue::from_str(url));
        }
        if urls.length() == 0 {
            continue;
        }
        let entry = web_sys::RtcIceServer::new();
        entry.set_urls(&urls);
        if let Some(username) = &server.username {
            entry.set_username(username);
        }
        if let Some(credential) = &server.credential {
            entry.set_credential(credential);
        }
        servers.push(&entry);
    }
    config.set_ice_servers(&servers);
    if use_relay == Some(true) {
        config.set_ice_transport_policy(RtcIceTransportPolicy::Relay);
    }
    let pc = RtcPeerConnection::new_with_configuration(&config)
        .map_err(|e| jserr("create peer connection", e))?;

    let mut closures = Vec::new();
    let (event_tx, events) = mpsc::unbounded::<PeerEvent>();

    // Both channels are negotiated on fixed stream ids, so both sides
    // just create them — no in-band open announcement. The stream id is
    // what's load-bearing against the desktop peer; the labels match
    // for sanity.
    let control_init = RtcDataChannelInit::new();
    control_init.set_negotiated(true);
    control_init.set_id(channel_spec::CONTROL_STREAM_ID);
    let control = pc
        .create_data_channel_with_data_channel_dict(channel_spec::CONTROL_LABEL, &control_init);

    let in_match_init = RtcDataChannelInit::new();
    in_match_init.set_negotiated(true);
    in_match_init.set_id(channel_spec::IN_MATCH_STREAM_ID);
    in_match_init.set_ordered(false);
    in_match_init.set_max_retransmits(0);
    let in_match = pc
        .create_data_channel_with_data_channel_dict(channel_spec::IN_MATCH_LABEL, &in_match_init);

    let (control_rx, control_open) = wire_channel(&control, &mut closures, true);
    let (in_match_rx, _) = wire_channel(&in_match, &mut closures, false);

    {
        let event_tx = event_tx.clone();
        let onicecandidate: Closure<dyn FnMut(web_sys::Event)> =
            Closure::new(move |e: web_sys::Event| {
                let e: web_sys::RtcPeerConnectionIceEvent = e.unchecked_into();
                if let Some(candidate) = e.candidate() {
                    let candidate = candidate.candidate();
                    // The empty candidate is end-of-candidates; peers
                    // don't need it.
                    if !candidate.is_empty() {
                        let _ = event_tx.unbounded_send(PeerEvent::Candidate(candidate));
                    }
                }
            });
        pc.set_onicecandidate(Some(onicecandidate.as_ref().unchecked_ref()));
        closures.push(onicecandidate);
    }
    {
        let event_tx = event_tx.clone();
        let pc2 = pc.clone();
        let onstate: Closure<dyn FnMut(web_sys::Event)> = Closure::new(move |_| {
            match pc2.connection_state() {
                RtcPeerConnectionState::Connected => {
                    let _ = event_tx.unbounded_send(PeerEvent::Connected);
                }
                RtcPeerConnectionState::Failed | RtcPeerConnectionState::Closed => {
                    let _ = event_tx.unbounded_send(PeerEvent::Failed);
                }
                // "disconnected" can self-heal; let ICE keep trying.
                _ => {}
            }
        });
        pc.set_onconnectionstatechange(Some(onstate.as_ref().unchecked_ref()));
        closures.push(onstate);
    }

    Ok(PeerParts {
        pc: PeerConnection {
            pc,
            _closures: closures,
        },
        events,
        control_tx: ChannelSender { dc: control },
        control_rx,
        in_match_tx: ChannelSender { dc: in_match },
        in_match_rx,
        control_open,
    })
}

/// Hook one datachannel's callbacks up: messages into an unbounded
/// channel (closed on channel close), plus an open barrier.
fn wire_channel(
    dc: &RtcDataChannel,
    closures: &mut Vec<Closure<dyn FnMut(web_sys::Event)>>,
    want_open: bool,
) -> (ChannelReceiver, oneshot::Receiver<()>) {
    dc.set_binary_type(RtcDataChannelType::Arraybuffer);
    let (tx, rx) = mpsc::unbounded::<Vec<u8>>();
    {
        let tx = tx.clone();
        let onmessage: Closure<dyn FnMut(web_sys::Event)> =
            Closure::new(move |e: web_sys::Event| {
                let e: web_sys::MessageEvent = e.unchecked_into();
                if let Ok(buf) = e.data().dyn_into::<js_sys::ArrayBuffer>() {
                    let _ = tx.unbounded_send(js_sys::Uint8Array::new(&buf).to_vec());
                }
            });
        dc.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        closures.push(onmessage);
    }
    {
        let onclose: Closure<dyn FnMut(web_sys::Event)> = Closure::new(move |_| {
            tx.close_channel();
        });
        dc.set_onclose(Some(onclose.as_ref().unchecked_ref()));
        closures.push(onclose);
    }
    let (open_tx, open_rx) = oneshot::channel::<()>();
    if want_open {
        let open_tx = Rc::new(RefCell::new(Some(open_tx)));
        let onopen: Closure<dyn FnMut(web_sys::Event)> = Closure::new(move |_| {
            if let Some(tx) = open_tx.borrow_mut().take() {
                let _ = tx.send(());
            }
        });
        dc.set_onopen(Some(onopen.as_ref().unchecked_ref()));
        closures.push(onopen);
    }
    (ChannelReceiver { rx }, open_rx)
}

impl PeerConnection {
    /// Create and install the local offer; returns its SDP for the
    /// signaling relay (candidates trickle separately).
    pub async fn create_offer(&self) -> anyhow::Result<String> {
        let offer = JsFuture::from(self.pc.create_offer())
            .await
            .map_err(|e| jserr("create offer", e))?;
        JsFuture::from(
            self.pc
                .set_local_description(offer.unchecked_ref::<RtcSessionDescriptionInit>()),
        )
        .await
        .map_err(|e| jserr("set local offer", e))?;
        self.local_sdp()
    }

    /// Perfect negotiation, polite side: roll our un-answered offer
    /// back, install the peer's offer, and produce our installed
    /// answer's SDP.
    pub async fn rollback_and_accept_offer(&self, sdp: &str) -> anyhow::Result<String> {
        let rollback = RtcSessionDescriptionInit::new(RtcSdpType::Rollback);
        JsFuture::from(self.pc.set_local_description(&rollback))
            .await
            .map_err(|e| jserr("rollback local description", e))?;
        self.set_remote(RtcSdpType::Offer, sdp).await?;
        let answer = JsFuture::from(self.pc.create_answer())
            .await
            .map_err(|e| jserr("create answer", e))?;
        JsFuture::from(
            self.pc
                .set_local_description(answer.unchecked_ref::<RtcSessionDescriptionInit>()),
        )
        .await
        .map_err(|e| jserr("set local answer", e))?;
        self.local_sdp()
    }

    pub async fn accept_answer(&self, sdp: &str) -> anyhow::Result<()> {
        self.set_remote(RtcSdpType::Answer, sdp).await
    }

    async fn set_remote(&self, sdp_type: RtcSdpType, sdp: &str) -> anyhow::Result<()> {
        let desc = RtcSessionDescriptionInit::new(sdp_type);
        desc.set_sdp(sdp);
        JsFuture::from(self.pc.set_remote_description(&desc))
            .await
            .map_err(|e| jserr("set remote description", e))?;
        Ok(())
    }

    fn local_sdp(&self) -> anyhow::Result<String> {
        Ok(self
            .pc
            .local_description()
            .ok_or_else(|| anyhow::anyhow!("local description missing"))?
            .sdp())
    }

    /// This side's DTLS certificate fingerprint (raw SHA-256 bytes),
    /// parsed from the local SDP. Empty if unparsable — callers must
    /// tolerate that (the reconnect id has a seed-only fallback).
    pub fn local_dtls_fingerprint(&self) -> Vec<u8> {
        self.pc
            .local_description()
            .and_then(|d| parse_dtls_fingerprint(&d.sdp()))
            .unwrap_or_default()
    }

    /// The peer's DTLS certificate fingerprint from the remote SDP.
    pub fn peer_dtls_fingerprint(&self) -> Vec<u8> {
        self.pc
            .remote_description()
            .and_then(|d| parse_dtls_fingerprint(&d.sdp()))
            .unwrap_or_default()
    }

    pub async fn add_remote_candidate(&self, candidate: &str) -> anyhow::Result<()> {
        let init = RtcIceCandidateInit::new(candidate);
        // Datachannel-only SDPs have a single m-line.
        init.set_sdp_m_line_index(Some(0));
        JsFuture::from(
            self.pc
                .add_ice_candidate_with_opt_rtc_ice_candidate_init(Some(&init)),
        )
        .await
        .map_err(|e| jserr("add ice candidate", e))?;
        Ok(())
    }

    #[allow(dead_code)] // session teardown (M4)
    pub fn close(&self) {
        self.pc.close();
    }
}

impl Drop for PeerConnection {
    fn drop(&mut self) {
        self.pc.set_onicecandidate(None);
        self.pc.set_onconnectionstatechange(None);
        self.pc.close();
    }
}

/// First sha-256 `a=fingerprint:` line of an SDP as raw digest bytes —
/// the same parse the desktop's signaling client does.
fn parse_dtls_fingerprint(sdp: &str) -> Option<Vec<u8>> {
    for line in sdp.lines() {
        let Some(rest) = line.trim().strip_prefix("a=fingerprint:") else {
            continue;
        };
        let mut parts = rest.splitn(2, ' ');
        let algo = parts.next()?;
        if !algo.eq_ignore_ascii_case("sha-256") {
            continue;
        }
        let Some(hex) = parts.next() else { continue };
        let bytes: Option<Vec<u8>> = hex
            .split(':')
            .map(|octet| u8::from_str_radix(octet.trim(), 16).ok())
            .collect();
        match bytes {
            Some(b) if !b.is_empty() => return Some(b),
            _ => continue,
        }
    }
    None
}
