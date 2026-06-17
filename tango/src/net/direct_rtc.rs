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
//! Two data channels are pre-negotiated on fixed stream ids so there's no
//! in-band DCEP handshake either: a reliable/ordered control channel (stream 0)
//! and an unreliable/unordered in-match channel (stream 1) for the live
//! `data::wire` datagrams. The peer connection must be kept alive by the caller
//! for the channels' lifetime (see `netplay::NegotiationOutput`).

use super::{Receiver, Sender};
use datachannel_wrapper::{LocalDescriptionInit, PeerConnection, RtcConfig, SdpType, SessionDescription};

/// Fixed local ICE ufrag for the host (offerer) side. Must be a valid
/// ICE ufrag (>= 4 chars); both peers know both values.
const UFRAG_HOST: &str = "tangoHost";
/// Fixed local ICE ufrag for the dialing (answerer) side.
const UFRAG_CLIENT: &str = "tangoClient";
/// Shared ICE pwd. Must be a valid ICE pwd (>= 22 chars). Both sides use
/// the same value — ICE only requires each peer to know the other's pwd,
/// and a fixed shared secret satisfies that without an exchange.
const ICE_PWD: &str = "tangoDirectNoSignalingPwd";

/// Both transport channels brought up by the direct link, plus the peer
/// connection that owns them (kept alive by the caller). The channel specs
/// (labels, stream ids, reliability) live in [`super::channel`].
pub struct DirectChannels {
    /// Reliable, ordered — the control/lobby `Packet` protocol.
    pub control: (Sender, Receiver),
    /// Unreliable, unordered — the in-match `data::wire` datagrams.
    pub in_match: (Sender, Receiver),
    pub peer_conn: PeerConnection,
}

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

/// Open both pre-negotiated data channels on a fresh peer connection (the
/// reliable control channel + the unreliable in-match channel) and split each
/// into our transport-agnostic Sender/Receiver, returning the peer connection
/// so the caller can keep it alive.
fn open_channels(mut pc: PeerConnection) -> DirectChannels {
    let (label, init) = super::channel::control_channel();
    let control_dc = pc
        .create_data_channel(label, init)
        .expect("create pre-negotiated control data channel");
    let (label, init) = super::channel::in_match_channel();
    let in_match_dc = pc
        .create_data_channel(label, init)
        .expect("create pre-negotiated in-match data channel");
    DirectChannels {
        control: super::datachannel::pair(control_dc),
        in_match: super::datachannel::pair(in_match_dc),
        peer_conn: pc,
    }
}

/// Host side: pin the UDP `port`, offer with fixed ICE creds, and accept
/// the dialer reflexively. Returns once the descriptions are set; the
/// channels open asynchronously and the first `send` blocks until they do.
pub async fn host(port: u16) -> std::io::Result<DirectChannels> {
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

    let mut channels = open_channels(pc);

    channels.peer_conn.set_local_description(
        SdpType::Offer,
        Some(&LocalDescriptionInit {
            ice_ufrag: Some(UFRAG_HOST.to_string()),
            ice_pwd: Some(ICE_PWD.to_string()),
        }),
    )?;
    // The dialer answers as the DTLS client (`active`); no candidate — we
    // learn its address from the incoming connectivity check.
    channels
        .peer_conn
        .set_remote_description(fabricate_sdp(SdpType::Answer, "active", UFRAG_CLIENT, None))?;

    Ok(channels)
}

/// Dialer side: fabricate the host's offer (carrying a host candidate for
/// `addr`), then answer with fixed ICE creds.
pub async fn connect(addr: &str) -> std::io::Result<DirectChannels> {
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

    let mut channels = open_channels(pc);

    // The host offers as `actpass`; we become the DTLS client by answering
    // `active`. Set the remote offer first, then generate our answer.
    channels.peer_conn.set_remote_description(fabricate_sdp(
        SdpType::Offer,
        "actpass",
        UFRAG_HOST,
        Some(&candidate),
    ))?;
    channels.peer_conn.set_local_description(
        SdpType::Answer,
        Some(&LocalDescriptionInit {
            ice_ufrag: Some(UFRAG_CLIENT.to_string()),
            ice_pwd: Some(ICE_PWD.to_string()),
        }),
    )?;

    Ok(channels)
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

    /// End-to-end: host + dialer bring up both channels from fabricated SDP
    /// alone (no signaling), run the real protocol-version handshake over the
    /// reliable channel, then round-trip a raw datagram over the unreliable
    /// in-match channel. Proves ICE + DTLS + SCTP all complete and that the
    /// second pre-negotiated (stream-1, unreliable) channel opens and carries
    /// traffic both ways.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn fabricated_sdp_round_trips() {
        // A high, unlikely-to-clash loopback port for the test host.
        let port = 24987;
        let addr = format!("127.0.0.1:{port}");
        let (host_res, conn_res) = tokio::join!(host(port), connect(&addr));
        let mut host_ch = host_res.expect("host setup");
        let mut conn_ch = conn_res.expect("connect setup");

        // `negotiate`'s first send blocks until the channel opens, so this
        // drives the whole ICE/DTLS bring-up. Guard with a timeout so a
        // failure surfaces as a panic rather than a hang.
        let handshake = async {
            tokio::try_join!(
                crate::net::negotiate(&mut host_ch.control.0, &mut host_ch.control.1),
                crate::net::negotiate(&mut conn_ch.control.0, &mut conn_ch.control.1),
            )
        };
        tokio::time::timeout(std::time::Duration::from_secs(15), handshake)
            .await
            .expect("handshake timed out — channel never opened")
            .expect("negotiate failed");

        // The unreliable in-match channel shares the same association, so it's
        // open by now too — round-trip a raw datagram each way.
        let in_match = async {
            host_ch.in_match.0.send_raw(b"ping-h2c").await?;
            conn_ch.in_match.0.send_raw(b"ping-c2h").await?;
            let got_at_conn = conn_ch.in_match.1.recv_raw().await?;
            let got_at_host = host_ch.in_match.1.recv_raw().await?;
            Ok::<_, std::io::Error>((got_at_conn, got_at_host))
        };
        let (got_at_conn, got_at_host) = tokio::time::timeout(std::time::Duration::from_secs(15), in_match)
            .await
            .expect("in-match datagram timed out — second channel never opened")
            .expect("in-match send/recv failed");
        assert_eq!(got_at_conn, b"ping-h2c");
        assert_eq!(got_at_host, b"ping-c2h");
    }
}
