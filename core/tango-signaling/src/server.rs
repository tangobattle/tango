use futures_util::{SinkExt, StreamExt, TryStreamExt};
use prost::Message;

struct Session {
    num_clients: usize,
    offer_sdp: String,
    sinks: Vec<
        futures_util::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
            tokio_tungstenite::tungstenite::Message,
        >,
    >,
}

pub struct Server {
    listener: tokio::net::TcpListener,
    sessions: std::sync::Arc<
        tokio::sync::Mutex<
            std::collections::HashMap<String, std::sync::Arc<tokio::sync::Mutex<Session>>>,
        >,
    >,
}

async fn handle_connection(
    sessions: std::sync::Arc<
        tokio::sync::Mutex<
            std::collections::HashMap<String, std::sync::Arc<tokio::sync::Mutex<Session>>>,
        >,
    >,
    raw_stream: tokio::net::TcpStream,
    addr: std::net::SocketAddr,
) -> anyhow::Result<()> {
    let (tx, mut rx) = tokio_tungstenite::accept_async(raw_stream).await?.split();
    let mut tx = Some(tx);
    let session_id = std::sync::Arc::new(tokio::sync::Mutex::new(None));
    let mut session = None;
    let mut me: usize = 0;

    let r = {
        let sessions = sessions.clone();
        let session_id = session_id.clone();
        (move || async move {
            loop {
                let msg = match rx.try_next().await? {
                    Some(tokio_tungstenite::tungstenite::Message::Binary(d)) => {
                        tango_protos::signaling::Packet::decode(bytes::Bytes::from(d))?
                    }
                    Some(m) => {
                        anyhow::bail!("unexpected message: {:?}", m);
                    }
                    None => {
                        break;
                    }
                };
                log::debug!("received message from {}: {:?}", addr, msg);
                match msg.which {
                    Some(tango_protos::signaling::packet::Which::Start(start)) => {
                        let mut sessions = sessions.lock().await;
                        session = Some(
                            sessions
                                .entry(start.session_id.clone())
                                .or_insert_with(|| {
                                    std::sync::Arc::new(tokio::sync::Mutex::new(Session {
                                        num_clients: 0,
                                        offer_sdp: start.offer_sdp.clone(),
                                        sinks: vec![],
                                    }))
                                })
                                .clone(),
                        );

                        let session = session.as_ref().unwrap();
                        let mut session = session.lock().await;
                        session.num_clients += 1;
                        *session_id.lock().await = Some(start.session_id.clone());
                        let offer_sdp = session.offer_sdp.to_string();

                        me = session.sinks.len();
                        session.sinks.push(tx.take().unwrap());

                        if me == 1 {
                            session.sinks[me]
                                .send(tokio_tungstenite::tungstenite::Message::Binary(
                                    tango_protos::signaling::Packet {
                                        which: Some(tango_protos::signaling::packet::Which::Offer(
                                            tango_protos::signaling::packet::Offer {
                                                sdp: offer_sdp,
                                            },
                                        )),
                                    }
                                    .encode_to_vec(),
                                ))
                                .await?;
                        }
                    }
                    Some(tango_protos::signaling::packet::Which::Offer(_)) => {
                        anyhow::bail!(
                            "received offer from client: only the server may send offers"
                        );
                    }
                    Some(tango_protos::signaling::packet::Which::Answer(answer)) => {
                        let session = match session.as_ref() {
                            Some(session) => session,
                            None => {
                                anyhow::bail!("no session active");
                            }
                        };
                        let mut session = session.lock().await;
                        session.sinks[0]
                            .send(tokio_tungstenite::tungstenite::Message::Binary(
                                tango_protos::signaling::Packet {
                                    which: Some(tango_protos::signaling::packet::Which::Answer(
                                        tango_protos::signaling::packet::Answer { sdp: answer.sdp },
                                    )),
                                }
                                .encode_to_vec(),
                            ))
                            .await?;
                    }
                    Some(tango_protos::signaling::packet::Which::IceCandidate(ice_candidate)) => {
                        let session = match session.as_ref() {
                            Some(session) => session,
                            None => {
                                anyhow::bail!("no session active");
                            }
                        };
                        let mut session = session.lock().await;
                        session.sinks[1 - me]
                            .send(tokio_tungstenite::tungstenite::Message::Binary(
                                tango_protos::signaling::Packet {
                                    which: Some(
                                        tango_protos::signaling::packet::Which::IceCandidate(
                                            tango_protos::signaling::packet::IceCandidate {
                                                candidate: ice_candidate.candidate,
                                                mid: ice_candidate.mid,
                                            },
                                        ),
                                    ),
                                }
                                .encode_to_vec(),
                            ))
                            .await?;
                    }
                    p => anyhow::bail!("unknown packet: {:?}", p),
                }
            }
            Ok(())
        })()
        .await
    };

    if let Some(session_id) = &*session_id.lock().await {
        let mut sessions = sessions.lock().await;
        let should_delete = {
            if let Some(session) = sessions.get(session_id) {
                let mut session = session.lock().await;
                session.num_clients -= 1;
                true
            } else {
                false
            }
        };

        if should_delete {
            sessions.remove(session_id);
        }
    }

    r
}

impl Server {
    pub fn new(listener: tokio::net::TcpListener) -> Server {
        Server {
            listener,
            sessions: std::sync::Arc::new(
                tokio::sync::Mutex::new(std::collections::HashMap::new()),
            ),
        }
    }

    pub async fn run(&mut self) {
        while let Ok((stream, addr)) = self.listener.accept().await {
            let sessions = self.sessions.clone();
            tokio::spawn(async move {
                if let Err(e) = handle_connection(sessions, stream, addr).await {
                    log::warn!("client {} disconnected with error: {}", addr, e);
                }
            });
        }
    }
}
