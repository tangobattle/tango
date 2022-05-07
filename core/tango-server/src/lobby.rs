use futures_util::{SinkExt, StreamExt, TryStreamExt};
use prost::Message;
use rand::Rng;

struct PendingPlayer {
    save_data: Option<Vec<u8>>,
    close_sender: Option<tokio::sync::oneshot::Sender<()>>,
    tx: futures_util::stream::SplitSink<
        hyper_tungstenite::WebSocketStream<hyper::upgrade::Upgraded>,
        tungstenite::Message,
    >,
}

struct Lobby {
    game_info: tango_protos::lobby::GameInfo,
    available_games: Vec<tango_protos::lobby::GameInfo>,
    settings: tango_protos::lobby::Settings,
    save_data: Vec<u8>,
    next_opponent_id: u32,
    pending_players: std::collections::HashMap<u32, PendingPlayer>,
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

            const START_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

            (move || async move {
                let msg = match tokio::time::timeout(START_TIMEOUT, rx.try_next()).await {
                    Ok(msg) => match msg? {
                        Some(tungstenite::Message::Binary(d)) => {
                            tango_protos::lobby::CreateStreamToServerMessage::decode(
                                bytes::Bytes::from(d),
                            )?
                        }
                        Some(tungstenite::Message::Close(_)) | None => {
                            return Ok(());
                        }
                        Some(m) => {
                            anyhow::bail!("unexpected message: {:?}", m);
                        }
                    },
                    Err(_) => {
                        tx.send(tungstenite::Message::Binary(tango_protos::lobby::CreateStreamToClientMessage {
                            which:
                                Some(tango_protos::lobby::create_stream_to_client_message::Which::DisconnectInd(
                                    tango_protos::lobby::create_stream_to_client_message::DisconnectIndication {
                                        reason: tango_protos::lobby::create_stream_to_client_message::disconnect_indication::Reason::StartTimeout.into(),
                                    }
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

                self.lobbies.lock().await.insert(
                    lobby_id.clone(),
                    Lobby {
                        game_info,
                        settings,
                        available_games: create_req.available_games,
                        save_data: create_req.save_data,
                        next_opponent_id: 0,
                        pending_players: std::collections::HashMap::new(),
                        creator_nickname: create_req.nickname,
                        creator_tx: tx,
                    },
                );

                *lobby_id_for_cleanup.lock().await = Some(lobby_id.clone());

                loop {
                    const WAIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60 * 5);

                    let msg = match tokio::time::timeout(WAIT_TIMEOUT, rx.try_next()).await {
                        Ok(msg) => match msg? {
                            Some(tungstenite::Message::Binary(d)) => {
                                tango_protos::lobby::CreateStreamToServerMessage::decode(
                                    bytes::Bytes::from(d),
                                )?
                            }
                            Some(tungstenite::Message::Close(_)) | None => {
                                return Ok(());
                            }
                            Some(m) => {
                                anyhow::bail!("unexpected message: {:?}", m);
                            }
                        },
                        Err(_) => {
                            let mut lobbies = self.lobbies.lock().await;
                            let lobby = if let Some(lobby) = lobbies.get_mut(&lobby_id) {
                                lobby
                            } else {
                                break;
                            };

                            lobby.creator_tx.send(tungstenite::Message::Binary(tango_protos::lobby::CreateStreamToClientMessage {
                                which:
                                    Some(tango_protos::lobby::create_stream_to_client_message::Which::DisconnectInd(
                                        tango_protos::lobby::create_stream_to_client_message::DisconnectIndication {
                                            reason: tango_protos::lobby::create_stream_to_client_message::disconnect_indication::Reason::WaitTimeout.into(),
                                        }
                                    )),
                            }.encode_to_vec())).await?;
                            anyhow::bail!("wait timed out");
                        },
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
                                break;
                            };

                            let pp = {
                                if let Some(pp) = lobby.pending_players.get_mut(&accept_req.opponent_id) {
                                    pp
                                } else {
                                    lobby.creator_tx.send(tungstenite::Message::Binary(tango_protos::lobby::CreateStreamToClientMessage {
                                        which:
                                            Some(tango_protos::lobby::create_stream_to_client_message::Which::AcceptResp(
                                                tango_protos::lobby::create_stream_to_client_message::AcceptResponse {
                                                    which: Some(tango_protos::lobby::create_stream_to_client_message::accept_response::Which::Error(tango_protos::lobby::create_stream_to_client_message::accept_response::Error{
                                                        reason: tango_protos::lobby::create_stream_to_client_message::accept_response::error::Reason::NoSuchOpponent.into(),
                                                    })),
                                                }
                                            )),
                                    }.encode_to_vec())).await?;
                                    continue;
                                }
                            };

                            let pp_save_data = match &pp.save_data {
                                None => {
                                    lobby.creator_tx.send(tungstenite::Message::Binary(tango_protos::lobby::CreateStreamToClientMessage {
                                        which:
                                            Some(tango_protos::lobby::create_stream_to_client_message::Which::AcceptResp(
                                                tango_protos::lobby::create_stream_to_client_message::AcceptResponse {
                                                    which: Some(tango_protos::lobby::create_stream_to_client_message::accept_response::Which::Error(tango_protos::lobby::create_stream_to_client_message::accept_response::Error{
                                                        reason: tango_protos::lobby::create_stream_to_client_message::accept_response::error::Reason::NoSuchOpponent.into(),
                                                    })),
                                                }
                                            )),
                                    }.encode_to_vec())).await?;
                                    continue;
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
                                            which: Some(tango_protos::lobby::create_stream_to_client_message::accept_response::Which::Ok(tango_protos::lobby::create_stream_to_client_message::accept_response::Ok{
                                                session_id: session_id.clone(),
                                                save_data: pp_save_data.clone(),
                                            })),
                                        }
                                    )),
                            }.encode_to_vec())).await?;


                            pp.tx.send(tungstenite::Message::Binary(tango_protos::lobby::JoinStreamToClientMessage {
                                which:
                                    Some(tango_protos::lobby::join_stream_to_client_message::Which::AcceptInd(
                                        tango_protos::lobby::join_stream_to_client_message::AcceptIndication {
                                            session_id,
                                            save_data: lobby.save_data.clone(),
                                        }
                                    )),
                            }.encode_to_vec())).await?;

                            if let Some(close_sender) = pp.close_sender.take() {
                                let _ = close_sender.send(());
                            }
                            break;
                        },

                        tango_protos::lobby::CreateStreamToServerMessage {
                            which:
                                Some(tango_protos::lobby::create_stream_to_server_message::Which::RejectReq(
                                    reject_req,
                                )),
                        } => {
                            let mut lobbies = self.lobbies.lock().await;
                            let lobby = if let Some(lobby) = lobbies.get_mut(&lobby_id) {
                                lobby
                            } else {
                                break;
                            };

                            let mut pp = if let Some(pp) = lobby.pending_players.remove(&reject_req.opponent_id) {
                                pp
                            } else {
                                // No such player, just continue.
                                continue;
                            };

                            lobby.creator_tx.send(tungstenite::Message::Binary(tango_protos::lobby::CreateStreamToClientMessage {
                                which:
                                    Some(tango_protos::lobby::create_stream_to_client_message::Which::RejectResp(
                                        tango_protos::lobby::create_stream_to_client_message::RejectResponse { }
                                    )),
                            }.encode_to_vec())).await?;

                            pp.tx.send(tungstenite::Message::Binary(tango_protos::lobby::JoinStreamToClientMessage {
                                which:
                                    Some(tango_protos::lobby::join_stream_to_client_message::Which::DisconnectInd(
                                        tango_protos::lobby::join_stream_to_client_message::DisconnectIndication {
                                            reason: tango_protos::lobby::join_stream_to_client_message::disconnect_indication::Reason::Rejected.into(),
                                        }
                                    )),
                            }.encode_to_vec())).await?;

                            if let Some(close_sender) = pp.close_sender.take() {
                                let _ = close_sender.send(());
                            }
                        },
                        m => anyhow::bail!("unexpected message: {:?}", m),
                    };
                }

                Ok(())
            })().await
        };

        if let Some(lobby_id) = &*lobby_id_for_cleanup.lock().await {
            let mut lobbies = self.lobbies.lock().await;
            let mut lobby = if let Some(lobby) = lobbies.remove(lobby_id) {
                lobby
            } else {
                return r;
            };

            for (_, pp) in &mut lobby.pending_players {
                pp.tx.send(tungstenite::Message::Binary(tango_protos::lobby::JoinStreamToClientMessage {
                    which:
                        Some(tango_protos::lobby::join_stream_to_client_message::Which::DisconnectInd(
                            tango_protos::lobby::join_stream_to_client_message::DisconnectIndication {
                                reason: tango_protos::lobby::join_stream_to_client_message::disconnect_indication::Reason::LobbyClosed.into(),
                            }
                        )),
                }.encode_to_vec())).await?;
                if let Some(close_sender) = pp.close_sender.take() {
                    let _ = close_sender.send(());
                }
            }
        }

        r
    }

    pub async fn handle_join_stream(
        &self,
        ws: hyper_tungstenite::WebSocketStream<hyper::upgrade::Upgraded>,
    ) -> anyhow::Result<()> {
        let (mut tx, mut rx) = ws.split();
        let lobby_and_opponent_id_for_cleanup = std::sync::Arc::new(tokio::sync::Mutex::new(None));

        let r = {
            let lobbies = self.lobbies.clone();
            let lobby_and_opponent_id_for_cleanup = lobby_and_opponent_id_for_cleanup.clone();

            const START_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

            (move || async move {
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
                                Some(tango_protos::lobby::join_stream_to_client_message::Which::DisconnectInd(
                                    tango_protos::lobby::join_stream_to_client_message::DisconnectIndication {
                                        reason: tango_protos::lobby::join_stream_to_client_message::disconnect_indication::Reason::StartTimeout.into(),
                                    }
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

                let (close_sender, close_receiver) = tokio::sync::oneshot::channel();
                let opponent_id = {
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

                    let opponent_id = lobby.next_opponent_id;
                    lobby.next_opponent_id += 1;
                    lobby.pending_players
                        .insert(opponent_id, PendingPlayer {
                            save_data: None,
                            tx,
                            close_sender: Some(close_sender),
                        });

                    *lobby_and_opponent_id_for_cleanup.lock().await =
                        Some((lobby_id.clone(), opponent_id));

                    opponent_id
                };

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
                        let mut lobbies = lobbies.lock().await;
                        let lobby = match lobbies.get_mut(&lobby_id) {
                            Some(lobby) => lobby,
                            None => {
                                anyhow::bail!("no such lobby");
                            }
                        };

                        let pp = match lobby.pending_players.get_mut(&opponent_id) {
                            Some(pp) => pp,
                            None => {
                                anyhow::bail!("no such player");
                            }
                        };

                        pp.tx.send(tungstenite::Message::Binary(tango_protos::lobby::JoinStreamToClientMessage {
                            which:
                                Some(tango_protos::lobby::join_stream_to_client_message::Which::DisconnectInd(
                                    tango_protos::lobby::join_stream_to_client_message::DisconnectIndication {
                                        reason: tango_protos::lobby::join_stream_to_client_message::disconnect_indication::Reason::StartTimeout.into(),
                                    }
                                )),
                        }.encode_to_vec())).await?;
                        anyhow::bail!("request timed out");
                    },
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

                    let pp = match lobby.pending_players.get_mut(&opponent_id) {
                        Some(pp) => pp,
                        None => {
                            anyhow::bail!("no such player");
                        }
                    };

                    pp.save_data = Some(propose_req.save_data);
                    {
                        lobby.creator_tx.send(tungstenite::Message::Binary(tango_protos::lobby::CreateStreamToClientMessage {
                            which:
                                Some(tango_protos::lobby::create_stream_to_client_message::Which::ProposeInd(
                                    tango_protos::lobby::create_stream_to_client_message::ProposeIndication {
                                        opponent_id,
                                        opponent_nickname: propose_req.nickname,
                                        game_info: propose_req.game_info,
                                    }
                                )),
                        }.encode_to_vec())).await?;

                        pp.tx.send(tungstenite::Message::Binary(tango_protos::lobby::JoinStreamToClientMessage {
                            which:
                                Some(tango_protos::lobby::join_stream_to_client_message::Which::ProposeResp(
                                    tango_protos::lobby::join_stream_to_client_message::ProposeResponse { }
                                )),
                        }.encode_to_vec())).await?;

                    }
                }

                close_receiver.await?;

                Ok(())
            })()
            .await
        };

        if let Some((lobby_id, opponent_id)) = &*lobby_and_opponent_id_for_cleanup.lock().await {
            let mut lobbies = self.lobbies.lock().await;
            let lobby = if let Some(lobby) = lobbies.get_mut(lobby_id) {
                lobby
            } else {
                return r;
            };
            lobby.pending_players.remove(opponent_id);
        }

        r
    }
}
