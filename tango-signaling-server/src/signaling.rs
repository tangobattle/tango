use futures_util::{SinkExt, StreamExt, TryStreamExt};
use prost::Message;

struct Session {
    offer_sdp: String,
    sinks: Vec<
        futures_util::stream::SplitSink<
            hyper_tungstenite::WebSocketStream<hyper::upgrade::Upgraded>,
            tungstenite::Message,
        >,
    >,
}

pub struct Server {
    sessions: std::sync::Arc<
        tokio::sync::Mutex<
            std::collections::HashMap<String, std::sync::Arc<tokio::sync::Mutex<Session>>>,
        >,
    >,
}

impl Server {
    pub fn new() -> Server {
        Server {
            sessions: std::sync::Arc::new(
                tokio::sync::Mutex::new(std::collections::HashMap::new()),
            ),
        }
    }

    pub async fn handle_stream(
        &self,
        ws: hyper_tungstenite::WebSocketStream<hyper::upgrade::Upgraded>,
        session_id: &str,
    ) -> anyhow::Result<()> {
        let (tx, mut rx) = ws.split();
        let session_id_for_cleanup = std::sync::Arc::new(tokio::sync::Mutex::new(None));

        let r = {
            let sessions = self.sessions.clone();
            let session_id_for_cleanup = session_id_for_cleanup.clone();
            (move || async move {
                let mut session = None;
                let mut tx = Some(tx);
                let mut me: usize = 0;

                loop {
                    let msg = match rx.try_next().await? {
                        Some(tungstenite::Message::Binary(d)) => {
                            tango_protos::signaling::Packet::decode(bytes::Bytes::from(d))?
                        }
                        Some(tungstenite::Message::Close(_)) | None => {
                            break;
                        }
                        Some(m) => {
                            anyhow::bail!("unexpected message: {:?}", m);
                        }
                    };
                    log::debug!("received message: {:?}", msg);
                    match msg.which {
                        Some(tango_protos::signaling::packet::Which::Start(start)) => {
                            let mut sessions = sessions.lock().await;
                            session = Some(if let Some(session) = sessions.remove(session_id) {
                                session
                            } else {
                                sessions
                                    .entry(session_id.to_string())
                                    .or_insert_with(|| {
                                        std::sync::Arc::new(tokio::sync::Mutex::new(Session {
                                            offer_sdp: start.offer_sdp.clone(),
                                            sinks: vec![],
                                        }))
                                    })
                                    .clone()
                            });

                            let session = if let Some(session) = session.as_ref() {
                                session
                            } else {
                                anyhow::bail!("no such session");
                            };
                            let mut session = session.lock().await;
                            *session_id_for_cleanup.lock().await = Some(session_id);
                            let offer_sdp = session.offer_sdp.to_string();

                            me = session.sinks.len();
                            let tx = if let Some(tx) = tx.take() {
                                tx
                            } else {
                                anyhow::bail!("attempted to take tx twice");
                            };
                            session.sinks.push(tx);

                            if me == 1 {
                                session.sinks[me]
                                    .send(tungstenite::Message::Binary(
                                        tango_protos::signaling::Packet {
                                            which: Some(
                                                tango_protos::signaling::packet::Which::Offer(
                                                    tango_protos::signaling::packet::Offer {
                                                        sdp: offer_sdp,
                                                    },
                                                ),
                                            ),
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
                                .send(tungstenite::Message::Binary(
                                    tango_protos::signaling::Packet {
                                        which: Some(
                                            tango_protos::signaling::packet::Which::Answer(
                                                tango_protos::signaling::packet::Answer {
                                                    sdp: answer.sdp,
                                                },
                                            ),
                                        ),
                                    }
                                    .encode_to_vec(),
                                ))
                                .await?;
                        }
                        Some(tango_protos::signaling::packet::Which::IceCandidate(
                            ice_candidate,
                        )) => {
                            let session = match session.as_ref() {
                                Some(session) => session,
                                None => {
                                    anyhow::bail!("no session active");
                                }
                            };
                            let mut session = session.lock().await;
                            session.sinks[1 - me]
                                .send(tungstenite::Message::Binary(
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

        if let Some(session_id) = *session_id_for_cleanup.lock().await {
            let mut sessions = self.sessions.lock().await;
            sessions.remove(session_id);
        }

        r
    }
}
