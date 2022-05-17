use futures_util::SinkExt;
use futures_util::TryStreamExt;
use prost::Message;

pub async fn connect(
    addr: &str,
    peer_conn: &mut datachannel_wrapper::PeerConnection,
    mut event_rx: tokio::sync::mpsc::Receiver<datachannel_wrapper::PeerConnectionEvent>,
    session_id: &str,
) -> Result<(), anyhow::Error>
where
{
    let (mut stream, _) = tokio_tungstenite::connect_async(addr).await?;

    log::info!("negotiation started");

    loop {
        if let Some(datachannel_wrapper::PeerConnectionEvent::GatheringStateChange(
            datachannel_wrapper::GatheringState::Complete,
        )) = event_rx.recv().await
        {
            break;
        }
    }

    let local_description = peer_conn.local_description().unwrap();
    stream
        .send(tokio_tungstenite::tungstenite::Message::Binary(
            tango_protos::signaling::Packet {
                which: Some(tango_protos::signaling::packet::Which::Start(
                    tango_protos::signaling::packet::Start {
                        session_id: session_id.to_string(),
                        offer_sdp: local_description.sdp.to_string(),
                    },
                )),
            }
            .encode_to_vec(),
        ))
        .await?;
    log::info!("negotiation start sent");

    loop {
        tokio::select! {
            signal_msg = event_rx.recv() => {
                let cand = if let Some(datachannel_wrapper::PeerConnectionEvent::IceCandidate(cand)) = signal_msg {
                    cand
                } else {
                    anyhow::bail!("ice candidate not received")
                };

                stream
                    .send(tokio_tungstenite::tungstenite::Message::Binary(
                        tango_protos::signaling::Packet {
                            which: Some(
                                tango_protos::signaling::packet::Which::IceCandidate(
                                    tango_protos::signaling::packet::IceCandidate {
                                        candidate: cand.candidate,
                                        mid: cand.mid,
                                    },
                                ),
                            ),
                        }
                        .encode_to_vec(),
            ))
                    .await?;
            }
            ws_msg = stream.try_next() => {
                let raw = if let Some(raw) = ws_msg? {
                    raw
                } else {
                    anyhow::bail!("stream ended early");
                };

                let packet = if let tokio_tungstenite::tungstenite::Message::Binary(d) = raw {
                    tango_protos::signaling::Packet::decode(bytes::Bytes::from(d))?
                } else {
                    anyhow::bail!("invalid packet");
                };

                match packet.which {
                    Some(tango_protos::signaling::packet::Which::Start(_)) => {
                        anyhow::bail!("unexpected start");
                    }
                    Some(tango_protos::signaling::packet::Which::Offer(offer)) => {
                        log::info!("received an offer, this is the polite side. rolling back our local description and switching to answer");

                        peer_conn.set_local_description(datachannel_wrapper::SdpType::Rollback)?;
                        peer_conn.set_remote_description(datachannel_wrapper::SessionDescription {
                            sdp_type: datachannel_wrapper::SdpType::Offer,
                            sdp: datachannel_wrapper::parse_sdp(&offer.sdp.to_string(), false)?,
                        })?;

                        let local_description = peer_conn.local_description().unwrap();
                        stream
                            .send(tokio_tungstenite::tungstenite::Message::Binary(
                                tango_protos::signaling::Packet {
                                    which: Some(tango_protos::signaling::packet::Which::Answer(
                                        tango_protos::signaling::packet::Answer { sdp: local_description.sdp.to_string() },
                                    )),
                                }
                                .encode_to_vec(),
                            ))
                            .await?;
                        log::info!("sent answer to impolite side");
                        break;
                    }
                    Some(tango_protos::signaling::packet::Which::Answer(answer)) => {
                        log::info!("received an answer, this is the impolite side");

                        peer_conn.set_remote_description(datachannel_wrapper::SessionDescription {
                            sdp_type: datachannel_wrapper::SdpType::Answer,
                            sdp: datachannel_wrapper::parse_sdp(&answer.sdp.to_string(), false)?,
                        })?;
                        break;
                    }
                    Some(tango_protos::signaling::packet::Which::IceCandidate(_ice_candidate)) => {
                        anyhow::bail!("ice candidates not supported");
                    }
                    p => {
                        anyhow::bail!("unexpected packet: {:?}", p);
                    }
                }
            }
        };
    }

    stream.close(None).await?;

    loop {
        match event_rx.recv().await {
            Some(signal) => match signal {
                datachannel_wrapper::PeerConnectionEvent::ConnectionStateChange(c) => match c {
                    datachannel_wrapper::ConnectionState::Connected => {
                        break;
                    }
                    datachannel_wrapper::ConnectionState::Disconnected => {
                        anyhow::bail!("peer connection unexpectedly disconnected");
                    }
                    datachannel_wrapper::ConnectionState::Failed => {
                        anyhow::bail!("peer connection failed");
                    }
                    datachannel_wrapper::ConnectionState::Closed => {
                        anyhow::bail!("peer connection unexpectedly closed");
                    }
                    _ => {}
                },
                _ => {}
            },
            None => unreachable!(),
        }
    }

    Ok(())
}
