use super::protocol;
use futures_util::SinkExt;
use futures_util::TryStreamExt;

#[derive(Eq, PartialEq, Clone, Copy)]
pub enum ConnectionSide {
    Polite,
    Impolite,
}

pub async fn connect<T, F, Fut>(
    addr: &str,
    make_peer_conn: F,
    session_id: &str,
) -> Result<
    (
        webrtc::peer_connection::RTCPeerConnection,
        T,
        ConnectionSide,
    ),
    anyhow::Error,
>
where
    Fut: std::future::Future<
        Output = anyhow::Result<(webrtc::peer_connection::RTCPeerConnection, T)>,
    >,
    F: Fn() -> Fut,
{
    let (mut stream, _) = tokio_tungstenite::connect_async(addr).await?;

    let mut side = ConnectionSide::Polite;

    log::info!("negotiation started");

    let (mut peer_conn, mut r) = make_peer_conn().await?;

    let mut gather_complete = peer_conn.gathering_complete_promise().await;
    let offer = peer_conn.create_offer(None).await?;
    peer_conn.set_local_description(offer).await?;
    gather_complete.recv().await;

    stream
        .send(tokio_tungstenite::tungstenite::Message::Binary(
            protocol::Packet::Start(protocol::Start {
                protocol_version: protocol::VERSION,
                session_id: session_id.to_string(),
                offer_sdp: peer_conn.local_description().await.expect("local sdp").sdp,
            })
            .serialize()?,
        ))
        .await?;
    log::info!("negotiation start sent");

    match match stream
        .try_next()
        .await?
        .ok_or(anyhow::format_err!("stream ended early"))?
    {
        tokio_tungstenite::tungstenite::Message::Binary(d) => protocol::Packet::deserialize(&d)?,
        _ => anyhow::bail!("unexpected message format"),
    } {
        protocol::Packet::Start(_) => {
            anyhow::bail!("unexpected start");
        }
        protocol::Packet::Offer(offer) => {
            log::info!("received an offer, this is the polite side");

            let (peer_conn2, r2) = make_peer_conn().await?;
            peer_conn = peer_conn2;
            r = r2;

            {
                let mut sdp = webrtc::peer_connection::sdp::session_description::RTCSessionDescription::default();
                sdp.sdp_type = webrtc::peer_connection::sdp::sdp_type::RTCSdpType::Offer;
                sdp.sdp = offer.sdp;
                peer_conn.set_remote_description(sdp).await?;
            }

            let mut gather_complete = peer_conn.gathering_complete_promise().await;
            let offer = peer_conn.create_answer(None).await?;
            peer_conn.set_local_description(offer).await?;
            gather_complete.recv().await;

            stream
                .send(tokio_tungstenite::tungstenite::Message::Binary(
                    protocol::Packet::Answer(protocol::Answer {
                        sdp: peer_conn.local_description().await.expect("remote sdp").sdp,
                    })
                    .serialize()?,
                ))
                .await?;
            log::info!("sent answer to impolite side");
        }
        protocol::Packet::Answer(answer) => {
            log::info!("received an answer, this is the impolite side");

            side = ConnectionSide::Impolite;
            let mut sdp =
                webrtc::peer_connection::sdp::session_description::RTCSessionDescription::default();
            sdp.sdp_type = webrtc::peer_connection::sdp::sdp_type::RTCSdpType::Answer;
            sdp.sdp = answer.sdp;
            peer_conn.set_remote_description(sdp).await?;
        }
        protocol::Packet::ICECandidate(_) => {
            anyhow::bail!("unexpected ice candidate");
        }
    }

    stream.close(None).await?;

    Ok((peer_conn, r, side))
}
