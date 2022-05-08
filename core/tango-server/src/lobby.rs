use futures_util::{SinkExt, StreamExt, TryStreamExt};
use prost::Message;
use rand::Rng;

struct Opponent {
    save_data: Vec<u8>,
    resp_tx: Option<tokio::sync::oneshot::Sender<tango_protos::lobby::JoinResponse>>,
}

struct Lobby {
    game_info: tango_protos::lobby::GameInfo,
    creator_nickname: String,
    available_games: Vec<tango_protos::lobby::GameInfo>,
    settings: tango_protos::lobby::Settings,
    opponent: Option<Opponent>,
    _close_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

pub struct Server {
    lobbies: std::sync::Arc<
        tokio::sync::Mutex<
            std::collections::HashMap<String, std::sync::Arc<tokio::sync::Mutex<Lobby>>>,
        >,
    >,
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
                let lobby = std::sync::Arc::new(tokio::sync::Mutex::new(Lobby {
                    game_info,
                    settings,
                    available_games: create_req.available_games,
                    opponent: None,
                    creator_nickname: create_req.nickname,
                    _close_tx: Some(close_tx),
                }));
                self.lobbies.lock().await.insert(
                    lobby_id.clone(),
                    lobby.clone(),
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
                                tx.send(tungstenite::Message::Binary(tango_protos::lobby::CreateStreamToClientMessage {
                                    which:
                                        Some(tango_protos::lobby::create_stream_to_client_message::Which::TimeoutInd(
                                            tango_protos::lobby::create_stream_to_client_message::TimeoutIndication { }
                                        )),
                                }.encode_to_vec())).await?;
                                anyhow::bail!("wait timed out");
                            },
                        }
                    };

                    let accept_req = match msg {
                        tango_protos::lobby::CreateStreamToServerMessage {
                            which:
                                Some(tango_protos::lobby::create_stream_to_server_message::Which::AcceptReq(
                                    accept_req,
                                )),
                        } => {
                            accept_req
                        },
                        m => anyhow::bail!("unexpected message: {:?}", m),
                    };

                    let mut opponent = {
                        let mut lobby = lobby.lock().await;
                        if let Some(opponent) = lobby.opponent.take() {
                            opponent
                        } else {
                            anyhow::bail!("no such opponent");
                        }
                    };

                    let opponent_tx = if let Some(opponent_tx) = opponent.resp_tx.take() {
                        opponent_tx
                    } else {
                        anyhow::bail!("no opponent tx");
                    };

                    let session_id = generate_id();

                    if let Err(_) = opponent_tx.send(tango_protos::lobby::JoinResponse {
                        session_id: session_id.clone(),
                        opponent_save_data: accept_req.save_data,
                    }) {
                        anyhow::bail!("the sender dropped");
                    }

                    tx.send(tungstenite::Message::Binary(tango_protos::lobby::CreateStreamToClientMessage {
                        which:
                            Some(tango_protos::lobby::create_stream_to_client_message::Which::AcceptResp(
                                tango_protos::lobby::create_stream_to_client_message::AcceptResponse {
                                    session_id: session_id.clone(),
                                    opponent_save_data: opponent.save_data,
                                }
                            )),
                    }.encode_to_vec())).await?;
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

    pub async fn handle_join_request(
        &self,
        req: tango_protos::lobby::JoinRequest,
    ) -> Result<tango_protos::lobby::JoinResponse, Error> {
        let lobby_id = req.lobby_id;
        let lobby = if let Some(lobby) = self.lobbies.lock().await.remove(&lobby_id) {
            lobby
        } else {
            return Err(Error::HTTPStatus(hyper::StatusCode::NOT_FOUND));
        };

        let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
        {
            let mut lobby = lobby.lock().await;
            lobby.opponent = Some(Opponent {
                save_data: req.save_data,
                resp_tx: Some(resp_tx),
            });
        }

        Ok(resp_rx.await.map_err(|e| Error::Anyhow(e.into()))?)
    }

    pub async fn handle_get_info_request(
        &self,
        req: tango_protos::lobby::GetInfoRequest,
    ) -> Result<tango_protos::lobby::GetInfoResponse, Error> {
        let lobbies = self.lobbies.lock().await;
        let lobby = if let Some(lobby) = lobbies.get(&req.lobby_id) {
            lobby
        } else {
            return Err(Error::HTTPStatus(hyper::StatusCode::NOT_FOUND));
        };

        let lobby = lobby.lock().await;

        Ok(tango_protos::lobby::GetInfoResponse {
            creator_nickname: lobby.creator_nickname.clone(),
            game_info: Some(lobby.game_info.clone()),
            available_games: lobby.available_games.clone(),
            settings: Some(lobby.settings.clone()),
        })
    }
}

#[derive(Debug)]
pub enum Error {
    HTTPStatus(hyper::StatusCode),
    Anyhow(anyhow::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for Error {}
