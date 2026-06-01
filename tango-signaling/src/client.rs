use futures_util::FutureExt;
use futures_util::SinkExt;
use futures_util::TryStreamExt;
use prost::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

pub type AbortReason = crate::proto::signaling::packet::abort::Reason;

/// Consecutive WebSocket-transport failures to ride out before giving up on
/// the matchmaking connection. The budget resets every time we re-reach the
/// server (its Hello arrives), so a reachable-but-flaky server keeps being
/// retried while a hard-down one still fails after this many tries.
const MAX_SIGNALING_RECONNECTS: u32 = 3;

/// Pause between automatic reconnects, so a hard-down server isn't hammered
/// in a tight burst.
const RECONNECT_DELAY: std::time::Duration = std::time::Duration::from_secs(1);

/// The matchmaking WebSocket stream type (what `connect_async` yields).
type SignalingStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn create_data_channel(
    rtc_config: datachannel_wrapper::RtcConfig,
) -> Result<
    (
        datachannel_wrapper::DataChannel,
        tokio::sync::mpsc::Receiver<datachannel_wrapper::PeerConnectionEvent>,
        datachannel_wrapper::PeerConnection,
    ),
    std::io::Error,
> {
    let (mut peer_conn, mut event_rx) = datachannel_wrapper::PeerConnection::new(rtc_config)?;

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

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("signaling abort: {0:?}")]
    ServerAbort(AbortReason),

    #[error("tungstenite: {0:?}")]
    Tungstenite(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("io: {0:?}")]
    Io(#[from] std::io::Error),

    #[error("prost decode error: {0:?}")]
    ProstDecode(#[from] prost::DecodeError),

    #[error("url parse error: {0:?}")]
    UrlParse(#[from] url::ParseError),

    #[error("http error: {0:?}")]
    Http(#[from] tokio_tungstenite::tungstenite::http::Error),

    #[error("sdp parse error: {0:?}")]
    SdpParse(#[from] datachannel_wrapper::sdp::error::SdpParserError),

    #[error("invalid packet")]
    InvalidPacket(tokio_tungstenite::tungstenite::Message),

    #[error("unexpected packet: {0:?}")]
    UnexpectedPacket(crate::proto::signaling::Packet),

    #[error("peer connection unexpectedly disconnected")]
    PeerConnectionDisconnected,

    #[error("peer connection failed")]
    PeerConnectionFailed,

    #[error("peer connection unexpectedly closed")]
    PeerConnectionClosed,
}

/// Whether an error is a WebSocket-transport drop — the connection to the
/// matchmaking server itself died — as opposed to a protocol / WebRTC failure
/// that re-running signaling wouldn't fix. Only these trigger a transparent
/// reconnect. `Tungstenite` is a raw tokio-tungstenite error; `Io` wraps the
/// stream-ended / read-timeout cases the handshake maps by hand — both mean
/// the socket to the server is gone.
fn is_websocket_disconnect(e: &Error) -> bool {
    matches!(e, Error::Tungstenite(_) | Error::Io(_))
}

pub struct Connecting {
    fut: futures_util::future::BoxFuture<
        'static,
        Result<(datachannel_wrapper::DataChannel, datachannel_wrapper::PeerConnection), Error>,
    >,
}

/// A connected-and-greeted matchmaking session, ready for the SDP exchange:
/// the live socket plus the freshly-built peer connection whose offer we've
/// already sent. Produced by [`signaling_prepare`], consumed by
/// [`signaling_exchange`].
struct Prepared {
    stream: SignalingStream,
    dc: datachannel_wrapper::DataChannel,
    event_rx: tokio::sync::mpsc::Receiver<datachannel_wrapper::PeerConnectionEvent>,
    peer_conn: datachannel_wrapper::PeerConnection,
}

pub async fn connect(
    addr: &str,
    session_id: &str,
    use_relay: Option<bool>,
    protocol_version: u32,
) -> Result<Connecting, Error> {
    // Connect + receive Hello eagerly so the caller gets the real
    // "connecting to server → waiting for opponent" transition (and a fast,
    // un-retried error if the server is unreachable at the outset).
    let prepared = signaling_prepare(addr, session_id, use_relay, protocol_version).await?;

    // Own the dial params so the future can re-run the handshake on reconnect.
    let addr = addr.to_owned();
    let session_id = session_id.to_owned();
    Ok(Connecting {
        fut: Box::pin(async move {
            // Run the SDP exchange, transparently reconnecting on a transport
            // drop so a matchmaking blip (e.g. the socket dying while we wait
            // for the opponent) never surfaces to the caller — no reconnection
            // churn upstream. The first exchange uses the eagerly-prepared
            // connection; each reconnect re-runs the whole handshake.
            //
            // `attempts` counts *consecutive* failures that don't re-reach the
            // server; `signaling_handshake` resets it the moment the server's
            // Hello arrives, so a reachable-but-flaky server keeps being
            // retried while a hard-down one gives up after
            // `MAX_SIGNALING_RECONNECTS`.
            let mut attempts: u32 = 0;
            let mut result = signaling_exchange(prepared).await;
            let (dc, mut event_rx, peer_conn) = loop {
                match result {
                    Ok(v) => break v,
                    Err(e) if is_websocket_disconnect(&e) && attempts < MAX_SIGNALING_RECONNECTS => {
                        attempts += 1;
                        log::warn!(
                            "signaling websocket dropped ({e}); reconnecting (attempt {attempts}/{MAX_SIGNALING_RECONNECTS})"
                        );
                        tokio::time::sleep(RECONNECT_DELAY).await;
                        result = signaling_handshake(&addr, &session_id, use_relay, protocol_version, &mut attempts).await;
                    }
                    Err(e) => return Err(e),
                }
            };

            // SDP exchanged and the socket is closed — the rest is pure WebRTC
            // ICE connectivity, so a failure here is a peer-connection problem,
            // never a matchmaking reconnect.
            loop {
                let signal = event_rx.recv().await.unwrap();

                if let datachannel_wrapper::PeerConnectionEvent::ConnectionStateChange(c) = signal {
                    match c {
                        datachannel_wrapper::ConnectionState::Connected => {
                            break;
                        }
                        datachannel_wrapper::ConnectionState::Disconnected => {
                            return Err(Error::PeerConnectionDisconnected);
                        }
                        datachannel_wrapper::ConnectionState::Failed => {
                            return Err(Error::PeerConnectionFailed);
                        }
                        datachannel_wrapper::ConnectionState::Closed => {
                            return Err(Error::PeerConnectionClosed);
                        }
                        _ => {}
                    }
                }
            }

            Ok((dc, peer_conn))
        }),
    })
}

/// A full reconnect attempt: re-establish the session ([`signaling_prepare`])
/// then run the SDP exchange ([`signaling_exchange`]). `*attempts` is reset to
/// 0 the moment the server's Hello arrives — re-reaching the server means the
/// reconnect budget should start fresh.
async fn signaling_handshake(
    addr: &str,
    session_id: &str,
    use_relay: Option<bool>,
    protocol_version: u32,
    attempts: &mut u32,
) -> Result<
    (
        datachannel_wrapper::DataChannel,
        tokio::sync::mpsc::Receiver<datachannel_wrapper::PeerConnectionEvent>,
        datachannel_wrapper::PeerConnection,
    ),
    Error,
> {
    let prepared = signaling_prepare(addr, session_id, use_relay, protocol_version).await?;
    // Re-reached the matchmaking server — refresh the reconnect budget.
    *attempts = 0;
    signaling_exchange(prepared).await
}

/// Open the WebSocket, receive the server's Hello (ICE config), build the peer
/// connection, and send our SDP offer. Stops short of waiting for the peer's
/// answer so the caller can surface a "waiting for opponent" state.
async fn signaling_prepare(
    addr: &str,
    session_id: &str,
    use_relay: Option<bool>,
    protocol_version: u32,
) -> Result<Prepared, Error> {
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
            "tango-signaling/{}",
            env!("CARGO_PKG_VERSION")
        ))
        .map_err(|e| tokio_tungstenite::tungstenite::http::Error::from(e))?,
    );
    req.headers_mut().append(
        "X-Tango-Protocol-Version",
        tokio_tungstenite::tungstenite::http::HeaderValue::from_str(&format!("{:x}", protocol_version))
            .map_err(|e| tokio_tungstenite::tungstenite::http::Error::from(e))?,
    );
    let mut signaling_stream = match tokio_tungstenite::connect_async(req).await {
        Ok((signaling_stream, _)) => signaling_stream,
        Err(tokio_tungstenite::tungstenite::Error::Http(e)) if e.status() == http::StatusCode::BAD_REQUEST => {
            let abort = crate::proto::signaling::packet::Abort::decode(
                e.body().as_ref().map(|b| b.as_bytes()).unwrap_or_default(),
            )?;
            return Err(Error::ServerAbort(
                AbortReason::try_from(abort.reason).unwrap_or_default(),
            ));
        }
        Err(e) => {
            return Err(e.into());
        }
    };

    let Some(raw) = signaling_stream.try_next().await? else {
        return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "stream ended early").into());
    };

    let packet = if let tokio_tungstenite::tungstenite::Message::Binary(d) = raw {
        crate::proto::signaling::Packet::decode(d.as_slice())?
    } else {
        return Err(Error::InvalidPacket(raw));
    };

    let Some(crate::proto::signaling::packet::Which::Hello(hello)) = packet.which else {
        return Err(Error::UnexpectedPacket(packet));
    };

    log::info!("hello received from signaling stream: {:?}", hello);

    let mut rtc_config = datachannel_wrapper::RtcConfig::new(
        &hello
            .ice_servers
            .into_iter()
            .flat_map(|ice_server| {
                ice_server
                    .urls
                    .into_iter()
                    .flat_map(|url| {
                        let Some(colon_idx) = url.chars().position(|c| c == ':') else {
                            return vec![];
                        };

                        let proto = &url[..colon_idx];
                        let rest = &url[colon_idx + 1..];

                        if (proto == "turn" || proto == "turns") && use_relay == Some(false) {
                            return vec![];
                        }

                        // libdatachannel doesn't support TURN over TCP: in fact, it explodes!
                        if url.chars().skip_while(|c| *c != '?').collect::<String>() == "?transport=tcp" {
                            return vec![];
                        }

                        if let (Some(username), Some(credential)) = (&ice_server.username, &ice_server.credential) {
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
    );
    if use_relay == Some(true) {
        rtc_config.ice_transport_policy = datachannel_wrapper::TransportPolicy::Relay;
    }
    let (dc, event_rx, peer_conn) = create_data_channel(rtc_config).await?;

    signaling_stream
        .send(tokio_tungstenite::tungstenite::Message::Binary(
            crate::proto::signaling::Packet {
                which: Some(crate::proto::signaling::packet::Which::Start(
                    crate::proto::signaling::packet::Start {
                        protocol_version,
                        offer_sdp: peer_conn.local_description().unwrap().sdp.to_string(),
                    },
                )),
            }
            .encode_to_vec(),
        ))
        .await?;

    Ok(Prepared {
        stream: signaling_stream,
        dc,
        event_rx,
        peer_conn,
    })
}

/// Drive the SDP offer/answer exchange to completion on an already-prepared
/// session, then close the socket. Returns the data channel + peer connection
/// ready for the ICE connectivity wait.
async fn signaling_exchange(
    prepared: Prepared,
) -> Result<
    (
        datachannel_wrapper::DataChannel,
        tokio::sync::mpsc::Receiver<datachannel_wrapper::PeerConnectionEvent>,
        datachannel_wrapper::PeerConnection,
    ),
    Error,
> {
    let Prepared {
        mut stream,
        dc,
        event_rx,
        mut peer_conn,
    } = prepared;

    const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
    const PING_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);
    let mut ping_interval = tokio::time::interval_at(tokio::time::Instant::now() + PING_INTERVAL, PING_INTERVAL);
    ping_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        let raw = tokio::select! {
            _ = ping_interval.tick() => {
                stream
                    .send(tokio_tungstenite::tungstenite::Message::Binary(
                        crate::proto::signaling::Packet {
                            which: Some(crate::proto::signaling::packet::Which::Ping(
                                crate::proto::signaling::packet::Ping {},
                            )),
                        }
                        .encode_to_vec(),
                    ))
                    .await?;
                continue;
            }
            result = tokio::time::timeout(READ_TIMEOUT, stream.try_next()) => {
                match result.map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "timed out"))?? {
                    Some(raw) => raw,
                    None => return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "stream ended early").into()),
                }
            }
        };

        let packet = match raw {
            tokio_tungstenite::tungstenite::Message::Binary(d) => {
                crate::proto::signaling::Packet::decode(d.as_slice())?
            }
            tokio_tungstenite::tungstenite::Message::Ping(_) => {
                // Note that upon receiving a ping message, tungstenite cues a pong reply automatically.
                // When you call either read_message, write_message or write_pending next it will try to send that pong out if the underlying connection can take more data.
                // This means you should not respond to ping frames manually.
                continue;
            }
            _ => {
                return Err(Error::InvalidPacket(raw));
            }
        };

        match &packet.which {
            Some(crate::proto::signaling::packet::Which::Ping(_)) => continue,
            Some(crate::proto::signaling::packet::Which::Abort(abort)) => {
                return Err(Error::ServerAbort(
                    AbortReason::try_from(abort.reason).unwrap_or_default(),
                ))
            }
            Some(crate::proto::signaling::packet::Which::Offer(offer)) => {
                log::info!("received an offer, this is the polite side. rolling back our local description and switching to answer");

                peer_conn.set_local_description(datachannel_wrapper::SdpType::Rollback)?;
                peer_conn.set_remote_description(datachannel_wrapper::SessionDescription {
                    sdp_type: datachannel_wrapper::SdpType::Offer,
                    sdp: datachannel_wrapper::sdp::parse_sdp(&offer.sdp.to_string(), false)?,
                })?;

                let local_description = peer_conn.local_description().unwrap();
                stream
                    .send(tokio_tungstenite::tungstenite::Message::Binary(
                        crate::proto::signaling::Packet {
                            which: Some(crate::proto::signaling::packet::Which::Answer(
                                crate::proto::signaling::packet::Answer {
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
            Some(crate::proto::signaling::packet::Which::Answer(answer)) => {
                log::info!("received an answer, this is the impolite side");

                peer_conn.set_remote_description(datachannel_wrapper::SessionDescription {
                    sdp_type: datachannel_wrapper::SdpType::Answer,
                    sdp: datachannel_wrapper::sdp::parse_sdp(&answer.sdp, false)?,
                })?;
                break;
            }
            _ => {
                return Err(Error::UnexpectedPacket(packet));
            }
        }
    }

    // SDP exchange done; closing the socket is best-effort — a close error
    // mustn't trigger a needless reconnect now that we have what we need.
    let _ = stream.close(None).await;

    log::debug!(
        "local sdp (type = {:?}): {}",
        peer_conn.local_description().expect("local sdp").sdp_type,
        peer_conn.local_description().expect("local sdp").sdp
    );
    log::debug!(
        "remote sdp (type = {:?}): {}",
        peer_conn.remote_description().expect("remote sdp").sdp_type,
        peer_conn.remote_description().expect("remote sdp").sdp
    );

    Ok((dc, event_rx, peer_conn))
}

impl std::future::Future for Connecting {
    type Output = Result<(datachannel_wrapper::DataChannel, datachannel_wrapper::PeerConnection), Error>;

    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        self.fut.poll_unpin(cx)
    }
}
