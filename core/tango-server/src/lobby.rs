use futures_util::{SinkExt, StreamExt, TryStreamExt};
use prost::Message;
use rand::Rng;

struct Opponent {
    nickname: String,
    save_data: Option<Vec<u8>>,
    _close_tx: Option<tokio::sync::oneshot::Sender<()>>,
    tx: futures_util::stream::SplitSink<
        hyper_tungstenite::WebSocketStream<hyper::upgrade::Upgraded>,
        tungstenite::Message,
    >,
}

struct Lobby {
    game_info: tango_protos::lobby::GameInfo,
    available_games: Vec<tango_protos::lobby::GameInfo>,
    settings: tango_protos::lobby::Settings,
    opponent: Option<Opponent>,
    _close_tx: Option<tokio::sync::oneshot::Sender<()>>,
    creator_nickname: String,
    creator_tx: futures_util::stream::SplitSink<
        hyper_tungstenite::WebSocketStream<hyper::upgrade::Upgraded>,
        tungstenite::Message,
    >,
}

pub struct Server {
    lobbies: std::sync::Arc<tokio::sync::Mutex<std::collections::HashMap<String, Lobby>>>,
}

fn generate_id() -> String {
    rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(32)
        .map(char::from)
        .collect()
}

impl Server {
    pub fn new() -> Server {
        Server {
            lobbies: std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
        }
    }

    pub async fn handle_create_stream(
        &self,
        ws: hyper_tungstenite::WebSocketStream<hyper::upgrade::Upgraded>,
    ) -> anyhow::Result<()> {
        let (mut tx, mut rx) = ws.split();
        let lobby_id_for_cleanup = std::sync::Arc::new(tokio::sync::Mutex::new(None));

        let r = {
            let lobby_id_for_cleanup = lobby_id_for_cleanup.clone();

            (move || async move {
                const START_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
                let msg = match tokio::time::timeout(START_TIMEOUT, rx.try_next()).await {
                    Ok(msg) => match msg? {
                        Some(tungstenite::Message::Binary(d)) => {
                            tango_protos::lobby::CreateStreamToServerMessage::decode(
                                bytes::Bytes::from(d),
                            )?
                        }
                        m => {
                            anyhow::bail!("unexpected message: {:?}", m);
                        }
                    },
                    Err(_) => {
                        tx.send(tungstenite::Message::Binary(tango_protos::lobby::CreateStreamToClientMessage {
                            which:
                                Some(tango_protos::lobby::create_stream_to_client_message::Which::TimeoutInd(
                                    tango_protos::lobby::create_stream_to_client_message::TimeoutIndication { }
                                )),
                        }.encode_to_vec())).await?;
                        anyhow::bail!("request timed out");
                    },
                };

                let create_req = match msg {
                    tango_protos::lobby::CreateStreamToServerMessage {
                        which:
                            Some(tango_protos::lobby::create_stream_to_server_message::Which::CreateReq(
                                create_req,
                            )),
                    } => create_req,
                    m => anyhow::bail!("unexpected message: {:?}", m),
                };

                let game_info = if let Some(game_info) = create_req.game_info {
                    game_info
                } else {
                    anyhow::bail!("create request was missing game info");
                };

                let settings = if let Some(settings) = create_req.settings {
                    settings
                } else {
                    anyhow::bail!("create request was missing settings");
                };

                let lobby_id = generate_id();

                tx.send(tungstenite::Message::Binary(tango_protos::lobby::CreateStreamToClientMessage {
                    which:
                        Some(tango_protos::lobby::create_stream_to_client_message::Which::CreateResp(
                            tango_protos::lobby::create_stream_to_client_message::CreateResponse {
                                lobby_id: lobby_id.clone(),
                            }
                        )),
                }.encode_to_vec())).await?;

                let (close_tx, close_rx) = tokio::sync::oneshot::channel();
                self.lobbies.lock().await.insert(
                    lobby_id.clone(),
                    Lobby {
                        game_info,
                        settings,
                        available_games: create_req.available_games,
                        opponent: None,
                        creator_nickname: create_req.nickname,
                        _close_tx: Some(close_tx),
                        creator_tx: tx,
                    },
                );

                *lobby_id_for_cleanup.lock().await = Some(lobby_id.clone());

                {
                    const WAIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60 * 5);
                    let msg = tokio::select! {
                        _ = close_rx => {
                            anyhow::bail!("lobby closed");
                        },
                        r = tokio::time::timeout(WAIT_TIMEOUT, rx.try_next()) => match r {
                            Ok(msg) => match msg? {
                                Some(tungstenite::Message::Binary(d)) => {
                                    tango_protos::lobby::CreateStreamToServerMessage::decode(
                                        bytes::Bytes::from(d),
                                    )?
                                }
                                m => {
                                    anyhow::bail!("unexpected message: {:?}", m);
                                }
                            },
                            Err(_) => {
                                let mut lobbies = self.lobbies.lock().await;
                                let lobby = if let Some(lobby) = lobbies.get_mut(&lobby_id) {
                                    lobby
                                } else {
                                    anyhow::bail!("no such lobby");
                                };

                                lobby.creator_tx.send(tungstenite::Message::Binary(tango_protos::lobby::CreateStreamToClientMessage {
                                    which:
                                        Some(tango_protos::lobby::create_stream_to_client_message::Which::TimeoutInd(
                                            tango_protos::lobby::create_stream_to_client_message::TimeoutIndication { }
                                        )),
                                }.encode_to_vec())).await?;
                                anyhow::bail!("wait timed out");
                            },
                        }
                    };

                    match msg {
                        tango_protos::lobby::CreateStreamToServerMessage {
                            which:
                                Some(tango_protos::lobby::create_stream_to_server_message::Which::AcceptReq(
                                    accept_req,
                                )),
                        } => {
                            let mut lobbies = self.lobbies.lock().await;
                            let lobby = if let Some(lobby) = lobbies.get_mut(&lobby_id) {
                                lobby
                            } else {
                                anyhow::bail!("no such lobby");
                            };

                            let opponent = {
                                if let Some(opponent) = lobby.opponent.as_mut() {
                                    opponent
                                } else {
                                    anyhow::bail!("no such opponent");
                                }
                            };

                            let opponent_save_data = match &opponent.save_data {
                                None => {
                                    anyhow::bail!("no save data");
                                },
                                Some(save_data) => {
                                    save_data.clone()
                                },
                            };

                            let session_id = generate_id();

                            lobby.creator_tx.send(tungstenite::Message::Binary(tango_protos::lobby::CreateStreamToClientMessage {
                                which:
                                    Some(tango_protos::lobby::create_stream_to_client_message::Which::AcceptResp(
                                        tango_protos::lobby::create_stream_to_client_message::AcceptResponse {
                                            session_id: session_id.clone(),
                                            opponent_save_data,
                                        }
                                    )),
                            }.encode_to_vec())).await?;

                            opponent.tx.send(tungstenite::Message::Binary(tango_protos::lobby::JoinStreamToClientMessage {
                                which:
                                    Some(tango_protos::lobby::join_stream_to_client_message::Which::ProposeResp(
                                        tango_protos::lobby::join_stream_to_client_message::ProposeResponse {
                                            session_id,
                                            opponent_save_data: accept_req.save_data,
                                        }
                                    )),
                            }.encode_to_vec())).await?;
                        },
                        m => anyhow::bail!("unexpected message: {:?}", m),
                    };
                }

                Ok(())
            })().await
        };

        if let Some(lobby_id) = &*lobby_id_for_cleanup.lock().await {
            let mut lobbies = self.lobbies.lock().await;
            lobbies.remove(lobby_id);
        }

        r
    }

    pub async fn handle_join_stream(
        &self,
        ws: hyper_tungstenite::WebSocketStream<hyper::upgrade::Upgraded>,
    ) -> anyhow::Result<()> {
        let (mut tx, mut rx) = ws.split();
        let lobby_id_for_cleanup = std::sync::Arc::new(tokio::sync::Mutex::new(None));

        let r = {
            let lobbies = self.lobbies.clone();
            let lobby_id_for_cleanup = lobby_id_for_cleanup.clone();

            (move || async move {
                const START_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
                let msg = match tokio::time::timeout(START_TIMEOUT, rx.try_next()).await {
                    Ok(msg) => match msg? {
                        Some(tungstenite::Message::Binary(d)) => {
                            tango_protos::lobby::JoinStreamToServerMessage::decode(bytes::Bytes::from(
                                d,
                            ))?
                        }
                        Some(tungstenite::Message::Close(_)) | None => {
                            return Ok(());
                        }
                        Some(m) => {
                            anyhow::bail!("unexpected message: {:?}", m);
                        }
                    },
                    Err(_) => {
                        tx.send(tungstenite::Message::Binary(tango_protos::lobby::JoinStreamToClientMessage {
                            which:
                                Some(tango_protos::lobby::join_stream_to_client_message::Which::TimeoutInd(
                                    tango_protos::lobby::join_stream_to_client_message::TimeoutIndication { }
                                )),
                        }.encode_to_vec())).await?;
                        anyhow::bail!("request timed out");
                    },
                };

                let join_req = match msg {
                    tango_protos::lobby::JoinStreamToServerMessage {
                        which:
                            Some(tango_protos::lobby::join_stream_to_server_message::Which::JoinReq(
                                join_req,
                            )),
                    } => join_req,
                    m => anyhow::bail!("unexpected message: {:?}", m),
                };

                let lobby_id = join_req.lobby_id;

                let (close_tx, close_rx) = tokio::sync::oneshot::channel();
                {
                    let mut lobbies = lobbies.lock().await;
                    let lobby = match lobbies.get_mut(&lobby_id) {
                        Some(lobby) => lobby,
                        None => {
                            tx.send(tungstenite::Message::Binary(tango_protos::lobby::JoinStreamToClientMessage {
                                which:
                                    Some(tango_protos::lobby::join_stream_to_client_message::Which::JoinResp(
                                        tango_protos::lobby::join_stream_to_client_message::JoinResponse {
                                            info: None,
                                        }
                                    )),
                            }.encode_to_vec())).await?;
                            anyhow::bail!("no such lobby");
                        }
                    };

                    tx.send(tungstenite::Message::Binary(tango_protos::lobby::JoinStreamToClientMessage {
                        which:
                            Some(tango_protos::lobby::join_stream_to_client_message::Which::JoinResp(
                                tango_protos::lobby::join_stream_to_client_message::JoinResponse {
                                    info: Some(tango_protos::lobby::join_stream_to_client_message::join_response::Info {
                                        opponent_nickname: lobby.creator_nickname.clone(),
                                        game_info: Some(lobby.game_info.clone()),
                                        available_games: lobby.available_games.clone(),
                                        settings: Some(lobby.settings.clone()),
                                    }),
                                }
                            )),
                    }.encode_to_vec())).await?;

                    lobby.opponent = Some(Opponent {
                        nickname: join_req.nickname,
                        save_data: None,
                        tx,
                        _close_tx: Some(close_tx),
                    });

                    *lobby_id_for_cleanup.lock().await = Some(lobby_id.clone());
                }

                const PROPOSE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60*5);
                let msg = tokio::select! {
                    _ = close_rx => {
                        anyhow::bail!("lobby closed");
                    },
                    r = tokio::time::timeout(PROPOSE_TIMEOUT, rx.try_next()) => match r {
                        Ok(msg) => match msg? {
                            Some(tungstenite::Message::Binary(d)) => {
                                tango_protos::lobby::JoinStreamToServerMessage::decode(bytes::Bytes::from(
                                    d,
                                ))?
                            }
                            Some(tungstenite::Message::Close(_)) | None => {
                                return Ok(());
                            }
                            Some(m) => {
                                anyhow::bail!("unexpected message: {:?}", m);
                            }
                        },
                        Err(_) => {
                            let mut lobbies = lobbies.lock().await;
                            let lobby = match lobbies.get_mut(&lobby_id) {
                                Some(lobby) => lobby,
                                None => {
                                    anyhow::bail!("no such lobby");
                                }
                            };

                            let opponent = match lobby.opponent.as_mut() {
                                Some(opponent) => opponent,
                                None => {
                                    anyhow::bail!("no such player");
                                }
                            };

                            opponent.tx.send(tungstenite::Message::Binary(tango_protos::lobby::JoinStreamToClientMessage {
                                which:
                                    Some(tango_protos::lobby::join_stream_to_client_message::Which::TimeoutInd(
                                        tango_protos::lobby::join_stream_to_client_message::TimeoutIndication { }
                                    )),
                            }.encode_to_vec())).await?;
                            anyhow::bail!("request timed out");
                        },
                    }
                };

                let propose_req = match msg {
                    tango_protos::lobby::JoinStreamToServerMessage {
                        which:
                            Some(tango_protos::lobby::join_stream_to_server_message::Which::ProposeReq(
                                propose_req,
                            )),
                    } => propose_req,
                    m => anyhow::bail!("unexpected message: {:?}", m),
                };

                {
                    let mut lobbies = lobbies.lock().await;
                    let lobby = match lobbies.get_mut(&lobby_id) {
                        Some(lobby) => lobby,
                        None => {
                            anyhow::bail!("no such lobby");
                        }
                    };

                    let opponent = match lobby.opponent.as_mut() {
                        Some(opponent) => opponent,
                        None => {
                            anyhow::bail!("no such player");
                        }
                    };

                    opponent.save_data = Some(propose_req.save_data);
                    {
                        lobby.creator_tx.send(tungstenite::Message::Binary(tango_protos::lobby::CreateStreamToClientMessage {
                            which:
                                Some(tango_protos::lobby::create_stream_to_client_message::Which::ProposeInd(
                                    tango_protos::lobby::create_stream_to_client_message::ProposeIndication {
                                        opponent_nickname: opponent.nickname.clone(),
                                        game_info: propose_req.game_info,
                                    }
                                )),
                        }.encode_to_vec())).await?;
                    }
                }

                Ok(())
            })()
            .await
        };

        if let Some(lobby_id) = &*lobby_id_for_cleanup.lock().await {
            let mut lobbies = self.lobbies.lock().await;
            lobbies.remove(lobby_id);
        }

        r
    }
}
