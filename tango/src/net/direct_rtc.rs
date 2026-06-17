//! Signaling-free WebRTC `DataChannel` transport for the direct
//! local-play link (`/host` and `/connect`). It replaces the old raw-TCP
//! adapter: instead of a TCP stream we bring up a real libdatachannel
//! peer connection, but with **no signaling exchange whatsoever**.
//!
//! Normally the two peers swap SDP (ICE ufrag/pwd + DTLS fingerprint +
//! candidates) through a signaling server. Here both sides instead
//! *fabricate* each other's description from constants they already
//! agree on:
//!
//! * **ICE credentials** are pinned to fixed values (see [`UFRAG_HOST`] /
//!   [`UFRAG_CLIENT`] / [`ICE_PWD`]) via libdatachannel's
//!   `LocalDescriptionInit`, so each side knows the other's ufrag/pwd up
//!   front and ICE connectivity checks validate.
//! * **The DTLS fingerprint** is unknowable without an exchange, so we
//!   disable fingerprint verification ([`RtcConfig::disable_fingerprint_verification`])
//!   and put a dummy (but well-formed) `sha-256` fingerprint in the
//!   fabricated SDP. The handshake still encrypts; it just doesn't pin
//!   the cert.
//! * **Addresses**: the host pins its UDP port (so the dialer knows where
//!   to send), and the dialer's fabricated offer carries a single host
//!   candidate for the typed `addr`. The host itself needs no remote
//!   candidate — it learns the dialer from the incoming STUN check
//!   (peer-reflexive).
//!
//! The data channel is pre-negotiated on a fixed stream id so there's no
//! in-band DCEP handshake either. The peer connection must be kept alive
//! by the caller for the channel's lifetime (see `netplay::NegotiationOutput`).

use super::{Receiver, Sender};
use datachannel_wrapper::{
    DataChannelInit, LocalDescriptionInit, PeerConnection, RtcConfig, SdpType, SessionDescription,
};

/// Fixed local ICE ufrag for the host (offerer) side. Must be a valid
/// ICE ufrag (>= 4 chars); both peers know both values.
const UFRAG_HOST: &str = "tangoHost";
/// Fixed local ICE ufrag for the dialing (answerer) side.
const UFRAG_CLIENT: &str = "tangoClient";
/// Shared ICE pwd. Must be a valid ICE pwd (>= 22 chars). Both sides use
/// the same value — ICE only requires each peer to know the other's pwd,
/// and a fixed shared secret satisfies that without an exchange.
const ICE_PWD: &str = "tangoDirectNoSignalingPwd";

/// Label + stream id of the pre-negotiated data channel. Both sides agree
/// on these so the channel exists without a DCEP open handshake.
const DC_LABEL: &str = "tango";
const DC_STREAM: u16 = 0;

/// Build a fabricated remote SDP for the peer. `setup` is the DTLS role
/// the *peer* advertises (`actpass` for an offer, `active` for an answer);
/// `ufrag` is the peer's pinned ICE ufrag. `candidate` is an optional
/// `a=candidate:` payload (host candidate for the dialer's view of the
/// host; `None` when the host learns the peer reflexively).
fn fabricate_sdp(sdp_type: SdpType, setup: &str, ufrag: &str, candidate: Option<&str>) -> SessionDescription {
    // sha-256 fingerprint: 32 colon-joined hex byte pairs (95 chars). The
    // value is a dummy — verification is disabled — but it must be
    // well-formed or the SDP parser rejects it.
    let fingerprint = ["AB"; 32].join(":");

    let mut lines = vec![
        "v=0".to_string(),
        "o=rtc 0 0 IN IP4 127.0.0.1".to_string(),
        "s=-".to_string(),
        "t=0 0".to_string(),
        "a=group:BUNDLE 0".to_string(),
        "a=msid-semantic:WMS *".to_string(),
        format!("a=fingerprint:sha-256 {fingerprint}"),
        "m=application 9 UDP/DTLS/SCTP webrtc-datachannel".to_string(),
        "c=IN IP4 0.0.0.0".to_string(),
        "a=mid:0".to_string(),
        "a=sendrecv".to_string(),
        "a=sctp-port:5000".to_string(),
        "a=max-message-size:262144".to_string(),
        format!("a=setup:{setup}"),
        format!("a=ice-ufrag:{ufrag}"),
        format!("a=ice-pwd:{ICE_PWD}"),
    ];
    if let Some(candidate) = candidate {
        lines.push(format!("a=candidate:{candidate}"));
    }
    lines.push("a=end-of-candidates".to_string());

    // libdatachannel parses on \r\n line endings; trailing CRLF too.
    let mut sdp = lines.join("\r\n");
    sdp.push_str("\r\n");
    SessionDescription { sdp_type, sdp }
}

/// Open the pre-negotiated data channel on a fresh peer connection and
/// split it into our transport-agnostic Sender/Receiver, returning the
/// peer connection so the caller can keep it alive.
fn open_channel(mut pc: PeerConnection) -> (Sender, Receiver, PeerConnection) {
    let dc = pc
        .create_data_channel(DC_LABEL, DataChannelInit::default().negotiated().stream(DC_STREAM))
        .expect("create pre-negotiated data channel");
    let (sender, receiver) = super::datachannel::pair(dc);
    (sender, receiver, pc)
}

/// Host side: pin the UDP `port`, offer with fixed ICE creds, and accept
/// the dialer reflexively. Returns once the descriptions are set; the
/// channel opens asynchronously and the first `send` blocks until it does.
pub async fn host(port: u16) -> std::io::Result<(Sender, Receiver, PeerConnection)> {
    let (pc, _events) = PeerConnection::new(RtcConfig {
        disable_fingerprint_verification: true,
        // We drive setLocalDescription ourselves (with pinned ICE creds);
        // an auto offer would race ahead with random creds.
        disable_auto_negotiation: true,
        // Pin the listen port so the dialer's fabricated host candidate
        // can target it.
        port_range: Some((port, port)),
        ..Default::default()
    })?;

    let (sender, receiver, mut pc) = open_channel(pc);

    pc.set_local_description(
        SdpType::Offer,
        Some(&LocalDescriptionInit {
            ice_ufrag: Some(UFRAG_HOST.to_string()),
            ice_pwd: Some(ICE_PWD.to_string()),
        }),
    )?;
    // The dialer answers as the DTLS client (`active`); no candidate — we
    // learn its address from the incoming connectivity check.
    pc.set_remote_description(fabricate_sdp(SdpType::Answer, "active", UFRAG_CLIENT, None))?;

    Ok((sender, receiver, pc))
}

/// Dialer side: fabricate the host's offer (carrying a host candidate for
/// `addr`), then answer with fixed ICE creds.
pub async fn connect(addr: &str) -> std::io::Result<(Sender, Receiver, PeerConnection)> {
    // Resolve the typed address into a concrete host candidate.
    let sock = tokio::net::lookup_host(addr)
        .await?
        .next()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "could not resolve address"))?;
    let candidate = host_candidate(&sock);

    let (pc, _events) = PeerConnection::new(RtcConfig {
        disable_fingerprint_verification: true,
        // We drive setLocalDescription ourselves (with pinned ICE creds);
        // an auto offer would race ahead with random creds and make us an
        // offerer instead of the answerer.
        disable_auto_negotiation: true,
        ..Default::default()
    })?;

    let (sender, receiver, mut pc) = open_channel(pc);

    // The host offers as `actpass`; we become the DTLS client by answering
    // `active`. Set the remote offer first, then generate our answer.
    pc.set_remote_description(fabricate_sdp(SdpType::Offer, "actpass", UFRAG_HOST, Some(&candidate)))?;
    pc.set_local_description(
        SdpType::Answer,
        Some(&LocalDescriptionInit {
            ice_ufrag: Some(UFRAG_CLIENT.to_string()),
            ice_pwd: Some(ICE_PWD.to_string()),
        }),
    )?;

    Ok((sender, receiver, pc))
}

/// Format an `a=candidate:` payload (everything after `a=candidate:`) for
/// a single UDP host candidate at `sock`.
fn host_candidate(sock: &std::net::SocketAddr) -> String {
    // foundation=1, component=1 (RTP), udp, an arbitrary host-typed
    // priority. libjuice resolves the rest from the connectivity check.
    format!("1 1 udp 2122260223 {} {} typ host", sock.ip(), sock.port())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// End-to-end: host + dialer bring up the channel from fabricated SDP
    /// alone (no signaling), then the real protocol-version handshake runs
    /// both ways. Proves ICE + DTLS + SCTP all complete and the channel is
    /// bidirectional.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn fabricated_sdp_round_trips() {
        // A high, unlikely-to-clash loopback port for the test host.
        let port = 24987;
        let addr = format!("127.0.0.1:{port}");
        let (host_res, conn_res) = tokio::join!(host(port), connect(&addr));
        let (mut host_tx, mut host_rx, _host_pc) = host_res.expect("host setup");
        let (mut conn_tx, mut conn_rx, _conn_pc) = conn_res.expect("connect setup");

        // `negotiate`'s first send blocks until the channel opens, so this
        // drives the whole ICE/DTLS bring-up. Guard with a timeout so a
        // failure surfaces as a panic rather than a hang.
        let handshake = async {
            tokio::try_join!(
                crate::net::negotiate(&mut host_tx, &mut host_rx),
                crate::net::negotiate(&mut conn_tx, &mut conn_rx),
            )
        };
        tokio::time::timeout(std::time::Duration::from_secs(15), handshake)
            .await
            .expect("handshake timed out — channel never opened")
            .expect("negotiate failed");
    }
}
