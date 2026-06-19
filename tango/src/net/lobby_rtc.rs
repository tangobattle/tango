//! Lobby-relayed WebRTC bring-up. Unlike the signaling-free [`super::direct_rtc`]
//! path (which fabricates SDP from fixed constants), this exchanges REAL SDP
//! offer/answer through the lobby server: we set our local description, gather
//! ICE candidates, hand the finished SDP to the caller to relay, and apply the
//! peer's SDP that comes back. The challenger offers, the accepter answers.
//!
//! The relay itself is the caller's job — `send_local_sdp` ships our SDP to the
//! peer (over the lobby's RtcOffer/RtcAnswer) and `sdp_rx` delivers theirs — so
//! this module stays free of any lobby/protocol types. The two data channels
//! and the resulting `PeerConnection` come back as [`super::channel::Channels`], the same
//! shape the direct path returns, so the rest of netplay treats both alike.

use datachannel_wrapper::{
    GatheringState, PeerConnection, PeerConnectionEvent, RtcConfig, SdpType, SessionDescription,
};

use super::channel::Channels;

/// Which half of the SDP exchange we drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LobbyRole {
    /// The challenger: create the offer, then apply the peer's answer.
    Offerer,
    /// The accepter: apply the peer's offer, then create the answer.
    Answerer,
}

/// Bring up the peer connection by relaying real SDP through the lobby.
///
/// `ice_servers` are formatted libdatachannel ICE URLs. `send_local_sdp` is
/// invoked exactly once with our finished local SDP (the caller relays it);
/// `sdp_rx` yields the peer's SDP exactly once.
pub async fn bring_up(
    ice_servers: Vec<String>,
    role: LobbyRole,
    use_relay: Option<bool>,
    send_local_sdp: impl FnOnce(String) + Send + 'static,
    mut sdp_rx: tokio::sync::mpsc::Receiver<String>,
) -> std::io::Result<Channels> {
    let mut config = RtcConfig {
        ice_servers,
        // We drive the offer/answer ourselves, after both channels exist (real
        // DTLS fingerprints are exchanged, so verification stays on).
        disable_auto_negotiation: true,
        ..Default::default()
    };
    // Relay-only when the user picked "Always". ("Never" is enforced upstream by
    // dropping the TURN servers from `ice_servers` before they reach us.)
    if use_relay == Some(true) {
        config.ice_transport_policy = datachannel_wrapper::TransportPolicy::Relay;
    }
    let (mut pc, mut events) = PeerConnection::new(config)?;

    // Pre-negotiate both channels on their fixed stream ids before any SDP, so
    // they ride the initial association (same as the signaling + direct paths).
    let (label, init) = super::channel::control_channel();
    let control_dc = pc.create_data_channel(label, init)?;
    let (label, init) = super::channel::in_match_channel();
    let in_match_dc = pc.create_data_channel(label, init)?;

    match role {
        LobbyRole::Offerer => {
            pc.set_local_description(SdpType::Offer, None)?;
            wait_for_gathering(&mut events).await?;
            send_local_sdp(local_sdp(&pc)?);
            let answer = recv_sdp(&mut sdp_rx).await?;
            pc.set_remote_description(SessionDescription {
                sdp_type: SdpType::Answer,
                sdp: answer,
            })?;
        }
        LobbyRole::Answerer => {
            let offer = recv_sdp(&mut sdp_rx).await?;
            pc.set_remote_description(SessionDescription {
                sdp_type: SdpType::Offer,
                sdp: offer,
            })?;
            pc.set_local_description(SdpType::Answer, None)?;
            wait_for_gathering(&mut events).await?;
            send_local_sdp(local_sdp(&pc)?);
        }
    }

    Ok(Channels {
        control: super::channel::pair(control_dc),
        in_match: super::channel::pair(in_match_dc),
        peer_conn: pc,
    })
}

/// Drive the event stream until ICE gathering completes — non-trickle, so the
/// local description we read afterwards carries all candidates.
async fn wait_for_gathering(
    events: &mut tokio::sync::mpsc::Receiver<PeerConnectionEvent>,
) -> std::io::Result<()> {
    loop {
        match events.recv().await {
            Some(PeerConnectionEvent::GatheringStateChange(GatheringState::Complete)) => {
                return Ok(())
            }
            Some(_) => continue,
            None => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "peer connection closed during ICE gathering",
                ))
            }
        }
    }
}

fn local_sdp(pc: &PeerConnection) -> std::io::Result<String> {
    pc.local_description()
        .map(|d| d.sdp)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "no local description"))
}

async fn recv_sdp(rx: &mut tokio::sync::mpsc::Receiver<String>) -> std::io::Result<String> {
    rx.recv()
        .await
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "lobby SDP relay closed"))
}
