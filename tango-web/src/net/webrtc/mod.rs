//! WebRTC peer transport, one backend per target with the same API:
//! tango's two negotiated fixed-stream-id datachannels (see
//! [`tango_net_protocol::channel_spec`] — "tango" id 0
//! reliable+ordered, "tango-match" id 1 unordered, zero retransmits;
//! rennet's redundancy replaces reliability), over
//! `web_sys::RtcPeerConnection` in the browser and libdatachannel
//! (`datachannel-wrapper`, the desktop client's stack) on native.
//! Sends are synchronous; receives pull from unbounded channels;
//! channel opens are explicit awaitable barriers.
//!
//! Both backends create the same two channels with the same ids in
//! negotiated mode, so the SCTP-level identity matches across targets
//! and no in-band DCEP handshake happens on either side.

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(not(target_arch = "wasm32"))]
pub use native::*;
#[cfg(target_arch = "wasm32")]
pub use web::*;

/// Connection-level events the signaling loop consumes.
pub enum PeerEvent {
    /// A trickled local ICE candidate to relay to the peer.
    Candidate(String),
    /// The connection came up.
    Connected,
    /// The connection failed or closed.
    Failed,
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
