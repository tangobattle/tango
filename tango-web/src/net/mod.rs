//! The web netplay transport: the signaling websocket ([`ws`]), the
//! browser `RTCPeerConnection` with tango's two negotiated data
//! channels ([`webrtc`]), the signaling choreography that pairs them
//! ([`signaling`]), and the control-plane packet transport
//! ([`control`]). The wire formats all come from the shared
//! `tango-net-protocol` crate — this module only moves bytes.

pub mod control;
pub mod signaling;
pub mod webrtc;
pub mod ws;
