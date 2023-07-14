use byteorder::WriteBytesExt;
use futures_util::{SinkExt, StreamExt, TryStreamExt};
use prost::Message;

use crate::iceconfig;

const ICECONFIG_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

struct Session {
    offer_sdp: String,
    offerer_tx: std::sync::Arc<
        tokio::sync::Mutex<
            futures_util::stream::SplitSink<
                hyper_tungstenite::WebSocketStream<hyper::upgrade::Upgraded>,
                tungstenite::Message,
            >,
        >,
    >,
}

pub struct Server {
    sessions: tokio::sync::Mutex<std::collections::HashMap<String, Session>>,
    iceconfig_backend: Option<Box<dyn iceconfig::Backend + Send + Sync + 'static>>,
}

impl Server {
    pub fn new(iceconfig_backend: Option<Box<dyn iceconfig::Backend + Send + Sync + 'static>>) -> Server {
        Server {
            sessions: tokio::sync::Mutex::new(std::collections::HashMap::new()),
            iceconfig_backend,
        }
    }

    pub async fn handle_stream(
        &self,
        ws: hyper_tungstenite::WebSocketStream<hyper::upgrade::Upgraded>,
        remote_ip: std::net::IpAddr,
        session_id: &str,
    ) -> anyhow::Result<()> {
        let r = self.handle_stream_inner(ws, remote_ip, session_id).await;
        let mut sessions = self.sessions.lock().await;
        sessions.remove(session_id);
        r
    }

    async fn handle_stream_inner(
        &self,
        ws: hyper_tungstenite::WebSocketStream<hyper::upgrade::Upgraded>,
        remote_ip: std::net::IpAddr,
        session_id: &str,
    ) -> anyhow::Result<()> {
        let (mut tx, mut rx) = ws.split();

        let ice_servers = if let Some(backend) = self.iceconfig_backend.as_ref() {
            match tokio::time::timeout(ICECONFIG_TIMEOUT, backend.get(&remote_ip))
                .await
                .map_err(|e| anyhow::Error::from(e))
                .and_then(|r| r)
            {
                Ok(ice_servers) => Some(ice_servers),
                Err(e) => {
                    log::error!("failed to request ICE servers: {:?}", e);
                    None
                }
            }
        } else {
            None
        };

        tx.send(tungstenite::Message::Binary(
            tango_signaling::proto::signaling::Packet {
                which: Some(tango_signaling::proto::signaling::packet::Which::Hello(
                    tango_signaling::proto::signaling::packet::Hello {
                        ice_servers: if let Some(ice_servers) = ice_servers {
                            ice_servers
                        } else {
                            vec![
                                tango_signaling::proto::signaling::packet::hello::IceServer {
                                    username: None,
                                    credential: None,
                                    urls: vec!["stun:stun.l.google.com:19302".to_string()],
                                },
                                tango_signaling::proto::signaling::packet::hello::IceServer {
                                    username: None,
                                    credential: None,
                                    urls: vec!["stun:stun1.l.google.com:19302".to_string()],
                                },
                                tango_signaling::proto::signaling::packet::hello::IceServer {
                                    username: None,
                                    credential: None,
                                    urls: vec!["stun:stun2.l.google.com:19302".to_string()],
                                },
                                tango_signaling::proto::signaling::packet::hello::IceServer {
                                    username: None,
                                    credential: None,
                                    urls: vec!["stun:stun3.l.google.com:19302".to_string()],
                                },
                                tango_signaling::proto::signaling::packet::hello::IceServer {
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

        const RX_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

        // Wait for start message.
        let start = match tokio::time::timeout(RX_TIMEOUT, rx.try_next())
            .await??
            .ok_or_else(|| anyhow::format_err!("unexpected end of stream"))?
        {
            tungstenite::Message::Binary(d) => {
                match tango_signaling::proto::signaling::Packet::decode(bytes::Bytes::from(d))?.which {
                    Some(tango_signaling::proto::signaling::packet::Which::Start(start)) => start,
                    m => anyhow::bail!("unexpected message: {:?}", m),
                }
            }
            m => {
                anyhow::bail!("unexpected message: {:?}", m);
            }
        };

        let tx = std::sync::Arc::new(tokio::sync::Mutex::new(tx));

        let offerer_tx = {
            let mut sessions = self.sessions.lock().await;
            if let Some(session) = sessions.remove(session_id) {
                tx.lock()
                    .await
                    .send(tungstenite::Message::Binary(
                        tango_signaling::proto::signaling::Packet {
                            which: Some(tango_signaling::proto::signaling::packet::Which::Offer(
                                tango_signaling::proto::signaling::packet::Offer {
                                    sdp: session.offer_sdp.clone(),
                                },
                            )),
                        }
                        .encode_to_vec(),
                    ))
                    .await?;

                Some(session.offerer_tx)
            } else {
                sessions.insert(
                    session_id.to_string(),
                    Session {
                        offer_sdp: start.offer_sdp,
                        offerer_tx: std::sync::Arc::clone(&tx),
                    },
                );
                None
            }
        };

        const PING_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
        let mut ping_timer = tokio::time::interval(PING_TIMEOUT);

        loop {
            tokio::select! {
                _ = ping_timer.tick() => {
                    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?;
                    let mut buf = vec![];
                    buf.write_u64::<byteorder::LittleEndian>(now.as_millis() as u64)?;
                    tx.lock().await.send(tungstenite::Message::Ping(buf)).await?;
                }

                msg = tokio::time::timeout(RX_TIMEOUT, rx.try_next()) => {
                    let answer = match msg?? {
                        Some(tungstenite::Message::Binary(d)) => {
                            match tango_signaling::proto::signaling::Packet::decode(bytes::Bytes::from(d))?.which {
                                Some(tango_signaling::proto::signaling::packet::Which::Answer(answer)) => answer,
                                m => anyhow::bail!("unexpected message: {:?}", m),
                            }
                        }
                        Some(tungstenite::Message::Pong(_)) => {
                            continue;
                        }
                        Some(tungstenite::Message::Close(_)) | None => {
                            return Ok(());
                        }
                        m => {
                            anyhow::bail!("unexpected message: {:?}", m);
                        }
                    };

                    let offerer_tx = if let Some(offerer_tx) = offerer_tx {
                        offerer_tx
                    } else {
                        anyhow::bail!("unexpected answer from offerer");
                    };

                    let mut offerer_tx = offerer_tx.lock().await;
                    offerer_tx
                        .send(tungstenite::Message::Binary(
                            tango_signaling::proto::signaling::Packet {
                                which: Some(tango_signaling::proto::signaling::packet::Which::Answer(
                                    tango_signaling::proto::signaling::packet::Answer { sdp: answer.sdp },
                                )),
                            }
                            .encode_to_vec(),
                        ))
                        .await?;
                    offerer_tx.close().await?;
                    return Ok(());
                }
            }
        }
    }
}
