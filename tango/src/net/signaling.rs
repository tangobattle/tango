use futures_util::SinkExt;
use futures_util::TryStreamExt;
use prost::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

async fn create_data_channel(
    ice_servers: &[String],
) -> Result<
    (
        datachannel_wrapper::DataChannel,
        tokio::sync::mpsc::Receiver<datachannel_wrapper::PeerConnectionEvent>,
        datachannel_wrapper::PeerConnection,
    ),
    anyhow::Error,
> {
    let (mut peer_conn, mut event_rx) =
        datachannel_wrapper::PeerConnection::new(datachannel_wrapper::RtcConfig::new(ice_servers))?;

    let dc = peer_conn.create_data_channel(
        "tango",
        datachannel_wrapper::DataChannelInit::default()
            .reliability(datachannel_wrapper::Reliability {
                unordered: false,
                unreliable: false,
                max_packet_life_time: 0,
                max_retransmits: 0,
            })
            .negotiated()
            .manual_stream()
            .stream(0),
    )?;

    loop {
        if let Some(datachannel_wrapper::PeerConnectionEvent::GatheringStateChange(
            datachannel_wrapper::GatheringState::Complete,
        )) = event_rx.recv().await
        {
            break;
        }
    }

    Ok((dc, event_rx, peer_conn))
}

pub async fn open(
    addr: &str,
    session_id: &str,
) -> Result<
    (
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        datachannel_wrapper::DataChannel,
        tokio::sync::mpsc::Receiver<datachannel_wrapper::PeerConnectionEvent>,
        datachannel_wrapper::PeerConnection,
    ),
    anyhow::Error,
> {
    let mut url = url::Url::parse(addr)?;
    url.set_query(Some(
        &url::form_urlencoded::Serializer::new(String::new())
            .append_pair("session_id", session_id)
            .finish(),
    ));

    let mut req = url.to_string().into_client_request()?;
    req.headers_mut().append(
        "User-Agent",
        tokio_tungstenite::tungstenite::http::HeaderValue::from_str(&format!(
            "tango/{}-{}",
            env!("CARGO_PKG_VERSION"),
            git_version::git_version!()
        ))?,
    );
    let (mut stream, _) = tokio_tungstenite::connect_async(req).await?;

    let raw = if let Some(raw) = stream.try_next().await? {
        raw
    } else {
        anyhow::bail!("stream ended early");
    };

    let packet = if let tokio_tungstenite::tungstenite::Message::Binary(d) = raw {
        tango_protos::matchmaking::Packet::decode(bytes::Bytes::from(d))?
    } else {
        anyhow::bail!("invalid packet");
    };

    let hello = if let Some(tango_protos::matchmaking::packet::Which::Hello(hello)) = packet.which {
        hello
    } else {
        anyhow::bail!("invalid packet");
    };

    let (dc, event_rx, peer_conn) = create_data_channel(
        &hello
            .ice_servers
            .into_iter()
            .flat_map(|ice_server| {
                ice_server
                    .urls
                    .into_iter()
                    .flat_map(|url| {
                        let colon_idx = if let Some(colon_idx) = url.chars().position(|c| c == ':')
                        {
                            colon_idx
                        } else {
                            return vec![];
                        };

                        let proto = &url[..colon_idx];
                        let rest = &url[colon_idx + 1..];

                        // libdatachannel doesn't support TURN over TCP: in fact, it explodes!
                        if url.chars().skip_while(|c| *c != '?').collect::<String>()
                            == "?transport=tcp"
                        {
                            return vec![];
                        }

                        if let (Some(username), Some(credential)) =
                            (&ice_server.username, &ice_server.credential)
                        {
                            vec![format!(
                                "{}:{}:{}@{}",
                                proto,
                                urlencoding::encode(username),
                                urlencoding::encode(credential),
                                rest
                            )]
                        } else {
                            vec![format!("{}:{}", proto, rest)]
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>(),
    )
    .await?;

    stream
        .send(tokio_tungstenite::tungstenite::Message::Binary(
            tango_protos::matchmaking::Packet {
                which: Some(tango_protos::matchmaking::packet::Which::Start(
                    tango_protos::matchmaking::packet::Start {
                        offer_sdp: peer_conn.local_description().unwrap().sdp.to_string(),
                    },
                )),
            }
            .encode_to_vec(),
        ))
        .await?;

    Ok((stream, dc, event_rx, peer_conn))
}

pub async fn connect(
    peer_conn: &mut datachannel_wrapper::PeerConnection,
    mut stream: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    mut event_rx: tokio::sync::mpsc::Receiver<datachannel_wrapper::PeerConnectionEvent>,
) -> Result<(), anyhow::Error> {
    loop {
        let raw = if let Some(raw) = stream.try_next().await? {
            raw
        } else {
            anyhow::bail!("stream ended early");
        };

        let packet = if let tokio_tungstenite::tungstenite::Message::Binary(d) = raw {
            tango_protos::matchmaking::Packet::decode(bytes::Bytes::from(d))?
        } else {
            anyhow::bail!("invalid packet");
        };

        match packet.which {
            Some(tango_protos::matchmaking::packet::Which::Start(_)) => {
                anyhow::bail!("unexpected start");
            }
            Some(tango_protos::matchmaking::packet::Which::Offer(offer)) => {
                log::info!("received an offer, this is the polite side. rolling back our local description and switching to answer");

                peer_conn.set_local_description(datachannel_wrapper::SdpType::Rollback)?;
                peer_conn.set_remote_description(datachannel_wrapper::SessionDescription {
                    sdp_type: datachannel_wrapper::SdpType::Offer,
                    sdp: datachannel_wrapper::sdp::parse_sdp(&offer.sdp.to_string(), false)?,
                })?;

                let local_description = peer_conn.local_description().unwrap();
                stream
                    .send(tokio_tungstenite::tungstenite::Message::Binary(
                        tango_protos::matchmaking::Packet {
                            which: Some(tango_protos::matchmaking::packet::Which::Answer(
                                tango_protos::matchmaking::packet::Answer {
                                    sdp: local_description.sdp.to_string(),
                                },
                            )),
                        }
                        .encode_to_vec(),
                    ))
                    .await?;
                log::info!("sent answer to impolite side");
                break;
            }
            Some(tango_protos::matchmaking::packet::Which::Answer(answer)) => {
                log::info!("received an answer, this is the impolite side");

                peer_conn.set_remote_description(datachannel_wrapper::SessionDescription {
                    sdp_type: datachannel_wrapper::SdpType::Answer,
                    sdp: datachannel_wrapper::sdp::parse_sdp(&answer.sdp, false)?,
                })?;
                break;
            }
            Some(tango_protos::matchmaking::packet::Which::IceCandidate(_ice_candidate)) => {
                anyhow::bail!("ice candidates not supported");
            }
            p => {
                anyhow::bail!("unexpected packet: {:?}", p);
            }
        }
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
