use byteorder::{ReadBytesExt, WriteBytesExt};
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use prost::Message;

#[derive(clap::Parser)]
struct Args {
    #[clap(long, default_value = "[::]:5432")]
    bind_addr: std::net::SocketAddr,

    #[clap(long, default_value = "1000")]
    max_users: u64,

    #[clap(long, default_value = "true")]
    use_x_real_ip: bool,
}

static CLIENT_VERSION_REQUIREMENT: once_cell::sync::Lazy<semver::VersionReq> =
    once_cell::sync::Lazy::new(|| semver::VersionReq::parse("*").unwrap());

async fn handle_request(
    server_state: std::sync::Arc<ServerState>,
    remote_ip: std::net::IpAddr,
    mut request: hyper::Request<hyper::Body>,
) -> Result<hyper::Response<hyper::Body>, anyhow::Error> {
    // Browsers cannot set headers on Websocket requests, so this prevents browsers from trivially opening up nettai connections.
    let client_version = if let Some(client_version) = request.headers().get("X-Nettai-Client-Version") {
        semver::Version::parse(client_version.to_str()?)?
    } else {
        return Ok(hyper::Response::builder()
            .status(hyper::StatusCode::BAD_REQUEST)
            .body(hyper::StatusCode::BAD_REQUEST.canonical_reason().unwrap().into())?);
    };

    if !CLIENT_VERSION_REQUIREMENT.matches(&client_version) {
        return Ok(hyper::Response::builder()
            .status(hyper::StatusCode::BAD_REQUEST)
            .body(hyper::StatusCode::BAD_REQUEST.canonical_reason().unwrap().into())?);
    }

    let token = if let Some(token) = request
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.split_once(' '))
        .and_then(|(scheme, token)| if scheme == "Nettai" { Some(token) } else { None })
    {
        token
    } else {
        return Ok(hyper::Response::builder()
            .status(hyper::StatusCode::UNAUTHORIZED)
            .body(hyper::StatusCode::UNAUTHORIZED.canonical_reason().unwrap().into())?);
    };

    if !hyper_tungstenite::is_upgrade_request(&request) {
        return Ok(hyper::Response::builder()
            .status(hyper::StatusCode::BAD_REQUEST)
            .body(hyper::StatusCode::BAD_REQUEST.canonical_reason().unwrap().into())?);
    }

    let (response, websocket) = hyper_tungstenite::upgrade(
        &mut request,
        Some(tungstenite::protocol::WebSocketConfig {
            max_message_size: Some(4 * 1024 * 1024),
            max_frame_size: Some(1 * 1024 * 1024),
            ..Default::default()
        }),
    )?;

    tokio::spawn(async move {
        let current_user_id = vec![]; // TODO

        if let Err(e) = {
            let server_state = server_state.clone();
            let current_user_id = current_user_id.clone();
            (|| async move {
                let websocket = websocket.await?;

                let (tx, rx) = websocket.split();
                let user_state = std::sync::Arc::new(UserState {
                    tx: Sender(tokio::sync::Mutex::new(tx)),
                    latencies: tokio::sync::Mutex::new(std::collections::VecDeque::new()),
                    ip: remote_ip,
                });

                // Broadcast connect.
                let _ = server_state
                    .broadcast_message(&nettai_client::protocol::Packet {
                        which: Some(nettai_client::protocol::packet::Which::Users(
                            nettai_client::protocol::packet::Users {
                                entries: vec![nettai_client::protocol::packet::users::Entry {
                                    user_id: current_user_id.clone(),
                                    info: Some(user_state.info()),
                                }],
                            },
                        )),
                    })
                    .await;

                {
                    let mut users = server_state.users.lock().await;
                    users.insert(current_user_id.clone(), user_state.clone());
                }
                handle_connection(
                    server_state.clone(),
                    current_user_id.as_slice(),
                    user_state.clone(),
                    Receiver(rx),
                )
                .await?;

                Ok::<_, anyhow::Error>(())
            })()
            .await
        } {
            log::error!("connection error for {}: {}", remote_ip, e);
        }

        server_state.users.lock().await.remove(&current_user_id);

        // Broadcast disconnect.
        let _ = server_state
            .broadcast_message(&nettai_client::protocol::Packet {
                which: Some(nettai_client::protocol::packet::Which::Users(
                    nettai_client::protocol::packet::Users {
                        entries: vec![nettai_client::protocol::packet::users::Entry {
                            user_id: current_user_id,
                            info: None,
                        }],
                    },
                )),
            })
            .await;
    });

    Ok(response)
}

struct Sender(
    tokio::sync::Mutex<
        futures_util::stream::SplitSink<
            hyper_tungstenite::WebSocketStream<hyper::upgrade::Upgraded>,
            tungstenite::Message,
        >,
    >,
);

impl Sender {
    async fn send(&self, message: tungstenite::Message) -> Result<(), tungstenite::Error> {
        self.0.lock().await.send(message).await
    }

    async fn send_binary(&self, buf: Vec<u8>) -> Result<(), tungstenite::Error> {
        self.0.lock().await.send(tungstenite::Message::Binary(buf)).await
    }

    async fn send_message(&self, msg: &impl prost::Message) -> Result<(), tungstenite::Error> {
        self.send_binary(msg.encode_to_vec()).await
    }
}

struct Receiver(futures_util::stream::SplitStream<hyper_tungstenite::WebSocketStream<hyper::upgrade::Upgraded>>);

impl Receiver {
    async fn recv(&mut self) -> Option<Result<tungstenite::Message, tungstenite::Error>> {
        self.0.next().await
    }
}

async fn handle_connection(
    server_state: std::sync::Arc<ServerState>,
    current_user_id: &[u8],
    user_state: std::sync::Arc<UserState>,
    mut rx: Receiver,
) -> Result<(), anyhow::Error> {
    // Send Hello.
    user_state
        .tx
        .send_message(&nettai_client::protocol::Packet {
            which: Some(nettai_client::protocol::packet::Which::Hello(
                nettai_client::protocol::packet::Hello {
                    user_id: current_user_id.to_vec(),
                },
            )),
        })
        .await?;

    // Send initial list of users.
    user_state
        .tx
        .send_message(&nettai_client::protocol::Packet {
            which: Some(nettai_client::protocol::packet::Which::Users(
                nettai_client::protocol::packet::Users {
                    entries: {
                        let users = server_state.users.lock().await;
                        users
                            .iter()
                            .map(|(user_id, user_state)| nettai_client::protocol::packet::users::Entry {
                                user_id: user_id.clone(),
                                info: Some(user_state.info()),
                            })
                            .collect()
                    },
                },
            )),
        })
        .await?;

    let mut ping_interval = tokio::time::interval(std::time::Duration::from_secs(30));

    loop {
        tokio::select! {
            msg = tokio::time::timeout(std::time::Duration::from_secs(60), rx.recv()) => {
                let msg = if let Some(msg) = msg? {
                    msg
                } else {
                    // Stream was closed.
                    return Ok(());
                }?;

                match msg {
                    tungstenite::Message::Binary(buf) => {
                        match nettai_client::protocol::Packet::decode(&mut bytes::Bytes::from(buf))?
                            .which
                            .ok_or_else(|| anyhow::anyhow!("unknown packet"))?
                        {
                            msg => {
                                return Err(anyhow::format_err!("unknown packet: {:?}", msg));
                            }
                        }
                    }

                    tungstenite::Message::Pong(buf) => {
                        let unix_ts_ms = buf.as_slice().read_u64::<byteorder::LittleEndian>()?;
                        let ts = std::time::UNIX_EPOCH + std::time::Duration::from_millis(unix_ts_ms);
                        let now = std::time::SystemTime::now();

                        // Record time.
                        let mut latencies = user_state.latencies.lock().await;
                        const MAX_LATENCIES: usize = 9;
                        while latencies.len() >= MAX_LATENCIES {
                            latencies.pop_front();
                        }
                        latencies.push_back(now.duration_since(ts)?);
                    }

                    _ => {
                        return Err(anyhow::anyhow!("cannot handle this message: {}", msg));
                    }
                }
            }

            _ = ping_interval.tick() => {
                let now = std::time::SystemTime::now();
                let unix_ts_ms = now.duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
                let mut buf = vec![];
                buf.write_u64::<byteorder::LittleEndian>(unix_ts_ms).unwrap();
                user_state.tx.send(tungstenite::Message::Ping(buf)).await?;
            }
        }
    }
}

struct UserState {
    tx: Sender,
    latencies: tokio::sync::Mutex<std::collections::VecDeque<std::time::Duration>>,
    ip: std::net::IpAddr,
}

impl UserState {
    fn info(&self) -> nettai_client::protocol::UserInfo {
        nettai_client::protocol::UserInfo {}
    }
}

struct ServerState {
    users: tokio::sync::Mutex<std::collections::HashMap<Vec<u8>, std::sync::Arc<UserState>>>,
}

impl ServerState {
    async fn broadcast_message(&self, msg: &impl prost::Message) -> Result<(), tungstenite::Error> {
        let users = self.users.lock().await;
        let raw = msg.encode_to_vec();
        futures_util::future::join_all(users.iter().map(|(_, u)| u.tx.send_binary(raw.clone())))
            .await
            .into_iter()
            .collect::<Result<_, _>>()?;
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    env_logger::Builder::from_default_env()
        .filter(Some("nettai_server"), log::LevelFilter::Info)
        .init();

    let args = Args::parse();

    let server_state = std::sync::Arc::new(ServerState {
        users: tokio::sync::Mutex::new(std::collections::HashMap::new()),
    });

    let incoming = hyper::server::conn::AddrIncoming::bind(&args.bind_addr)?;
    log::info!("listening on: {}", incoming.local_addr());

    let server = hyper::Server::builder(incoming).serve(hyper::service::make_service_fn(
        move |stream: &hyper::server::conn::AddrStream| {
            let server_state = server_state.clone();
            let raw_remote_ip = stream.remote_addr().ip();
            async move {
                Ok::<_, anyhow::Error>(hyper::service::service_fn(move |request| {
                    let server_state = server_state.clone();
                    async move {
                        let remote_ip = if args.use_x_real_ip {
                            if let Some(ip) = request
                                .headers()
                                .get("X-Real-IP")
                                .and_then(|h| h.to_str().ok())
                                .and_then(|v| v.parse().ok())
                            {
                                ip
                            } else {
                                return Err(anyhow::anyhow!("could not parse X-Real-IP"));
                            }
                        } else {
                            raw_remote_ip
                        };
                        handle_request(server_state, remote_ip, request).await
                    }
                }))
            }
        },
    ));
    server.await?;

    Ok(())
}
