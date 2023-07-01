use futures_util::{SinkExt, StreamExt, TryStreamExt};
use prost::Message;

use crate::iceconfig;

const ICECONFIG_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

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
        tokio::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<tokio::sync::Mutex<Session>>>>,
    >,
    iceconfig_backend: Option<Box<dyn iceconfig::Backend + Send + Sync + 'static>>,
}

impl Server {
    pub fn new(iceconfig_backend: Option<Box<dyn iceconfig::Backend + Send + Sync + 'static>>) -> Server {
        Server {
            sessions: std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            iceconfig_backend,
        }
    }

    pub async fn handle_stream(
        &self,
        ws: hyper_tungstenite::WebSocketStream<hyper::upgrade::Upgraded>,
        remote_ip: std::net::IpAddr,
        session_id: &str,
    ) -> anyhow::Result<()> {
        let (mut tx, mut rx) = ws.split();

        let ice_servers = if let Some(backend) = self.iceconfig_backend.as_ref() {
            match tokio::time::timeout(ICECONFIG_TIMEOUT, backend.get(&remote_ip)).await {
                Ok(Ok(ice_servers)) => Some(ice_servers),
                Err(_) => {
                    log::error!("requesting ICE servers timed out");
                    None
                }
                Ok(Err(e)) => {
                    log::error!("failed to request ICE servers: {:?}", e);
                    None
                }
            }
        } else {
            None
        };

        tx.send(tungstenite::Message::Binary(
            tango_protos::matchmaking::Packet {
                which: Some(tango_protos::matchmaking::packet::Which::Hello(
                    tango_protos::matchmaking::packet::Hello {
                        ice_servers: if let Some(ice_servers) = ice_servers {
                            ice_servers
                        } else {
                            vec![
                                tango_protos::matchmaking::packet::hello::IceServer {
                                    username: None,
                                    credential: None,
                                    urls: vec!["stun:stun.l.google.com:19302".to_string()],
                                },
                                tango_protos::matchmaking::packet::hello::IceServer {
                                    username: None,
                                    credential: None,
                                    urls: vec!["stun:stun1.l.google.com:19302".to_string()],
                                },
                                tango_protos::matchmaking::packet::hello::IceServer {
                                    username: None,
                                    credential: None,
                                    urls: vec!["stun:stun2.l.google.com:19302".to_string()],
                                },
                                tango_protos::matchmaking::packet::hello::IceServer {
                                    username: None,
                                    credential: None,
                                    urls: vec!["stun:stun3.l.google.com:19302".to_string()],
                                },
                                tango_protos::matchmaking::packet::hello::IceServer {
                                    username: None,
                                    credential: None,
                                    urls: vec!["stun:stun4.l.google.com:19302".to_string()],
                                },
                            ]
                        },
                    },
                )),
            }
            .encode_to_vec(),
        ))
        .await?;

        let session_id_for_cleanup = std::sync::Arc::new(tokio::sync::Mutex::new(None));

        let r = {
            let sessions = self.sessions.clone();
            let session_id_for_cleanup = session_id_for_cleanup.clone();
            (move || async move {
                let mut session = None;
                let mut tx = Some(tx);

                loop {
                    let msg = match rx.try_next().await? {
                        Some(tungstenite::Message::Binary(d)) => {
                            tango_protos::matchmaking::Packet::decode(bytes::Bytes::from(d))?
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
                        Some(tango_protos::matchmaking::packet::Which::Start(start)) => {
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

                            let me = session.sinks.len();
                            let tx = if let Some(tx) = tx.take() {
                                tx
                            } else {
                                anyhow::bail!("attempted to take tx twice");
                            };
                            session.sinks.push(tx);

                            if me == 1 {
                                session.sinks[me]
                                    .send(tungstenite::Message::Binary(
                                        tango_protos::matchmaking::Packet {
                                            which: Some(tango_protos::matchmaking::packet::Which::Offer(
                                                tango_protos::matchmaking::packet::Offer { sdp: offer_sdp },
                                            )),
                                        }
                                        .encode_to_vec(),
                                    ))
                                    .await?;
                            }
                        }
                        Some(tango_protos::matchmaking::packet::Which::Offer(_)) => {
                            anyhow::bail!("received offer from client: only the server may send offers");
                        }
                        Some(tango_protos::matchmaking::packet::Which::Answer(answer)) => {
                            let session = match session.as_ref() {
                                Some(session) => session,
                                None => {
                                    anyhow::bail!("no session active");
                                }
                            };
                            let mut session = session.lock().await;
                            session.sinks[0]
                                .send(tungstenite::Message::Binary(
                                    tango_protos::matchmaking::Packet {
                                        which: Some(tango_protos::matchmaking::packet::Which::Answer(
                                            tango_protos::matchmaking::packet::Answer { sdp: answer.sdp },
                                        )),
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
