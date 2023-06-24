mod httputil;

use byteorder::{ReadBytesExt, WriteBytesExt};
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use prost::Message;
use rand::seq::SliceRandom;
use routerify::ext::RequestExt;

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

    let remote_ip = if let Some(remote_ip) = request
        .data::<httputil::RealIPGetter>()
        .unwrap()
        .get_remote_real_ip(&request)
    {
        remote_ip
    } else {
        return Ok(hyper::Response::builder()
            .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
            .body(
                hyper::StatusCode::INTERNAL_SERVER_ERROR
                    .canonical_reason()
                    .unwrap()
                    .into(),
            )
            .unwrap());
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

    let server_state = request.data::<std::sync::Arc<ServerState>>().unwrap().clone();

    let current_user_id = if let Some(user_id) = server_state.available_ids.lock().await.pop_front() {
        user_id
    } else {
        return Ok(hyper::Response::builder()
            .status(hyper::StatusCode::SERVICE_UNAVAILABLE)
            .body(
                hyper::StatusCode::SERVICE_UNAVAILABLE
                    .canonical_reason()
                    .unwrap()
                    .into(),
            )?);
    };

    // No returns must occur between this and spawning, otherwise the allocated ID will be lost.

    tokio::spawn(async move {
        if let Err(e) = {
            let server_state = server_state.clone();
            (|| async move {
                let websocket = websocket.await?;

                let (tx, rx) = websocket.split();
                let user_state = std::sync::Arc::new(UserState {
                    tx: Sender(tokio::sync::Mutex::new(tx)),
                    latencies: tokio::sync::Mutex::new(std::collections::BinaryHeap::new()),
                    ip: remote_ip,
                });

                // Broadcast connect.
                let _ = server_state
                    .broadcast_message(nettai_client::protocol::Packet {
                        which: Some(nettai_client::protocol::packet::Which::Users(
                            nettai_client::protocol::packet::Users {
                                entries: vec![nettai_client::protocol::packet::users::Entry {
                                    user_id: current_user_id,
                                    info: Some(user_state.info()),
                                }],
                            },
                        )),
                    })
                    .await;

                {
                    let mut users = server_state.users.lock().await;
                    users.insert(current_user_id, user_state.clone());
                }
                handle_connection(server_state.clone(), current_user_id, user_state.clone(), Receiver(rx)).await?;

                Ok::<_, anyhow::Error>(())
            })()
            .await
        } {
            log::error!("connection error for {}: {}", remote_ip, e);
        }

        server_state.users.lock().await.remove(&current_user_id);
        server_state.available_ids.lock().await.push_back(current_user_id);

        // Broadcast disconnect.
        let _ = server_state
            .broadcast_message(nettai_client::protocol::Packet {
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

    async fn send_message(&self, msg: impl prost::Message) -> Result<(), tungstenite::Error> {
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
    current_user_id: u64,
    user_state: std::sync::Arc<UserState>,
    mut rx: Receiver,
) -> Result<(), anyhow::Error> {
    // Send Hello.
    user_state
        .tx
        .send_message(nettai_client::protocol::Packet {
            which: Some(nettai_client::protocol::packet::Which::Hello(
                nettai_client::protocol::packet::Hello {
                    user_id: current_user_id,
                },
            )),
        })
        .await?;

    // Send initial list of users.
    user_state
        .tx
        .send_message(nettai_client::protocol::Packet {
            which: Some(nettai_client::protocol::packet::Which::Users(
                nettai_client::protocol::packet::Users {
                    entries: {
                        let users = server_state.users.lock().await;
                        users
                            .iter()
                            .map(|(&user_id, user_state)| nettai_client::protocol::packet::users::Entry {
                                user_id,
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
                        latencies.shrink_to(MAX_LATENCIES);
                        latencies.push(now.duration_since(ts)?);
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
    latencies: tokio::sync::Mutex<std::collections::BinaryHeap<std::time::Duration>>,
    ip: std::net::IpAddr,
}

impl UserState {
    fn info(&self) -> nettai_client::protocol::UserInfo {
        nettai_client::protocol::UserInfo {}
    }
}

struct ServerState {
    available_ids: tokio::sync::Mutex<std::collections::VecDeque<u64>>,
    users: tokio::sync::Mutex<std::collections::HashMap<u64, std::sync::Arc<UserState>>>,
}

impl ServerState {
    async fn broadcast_message(&self, msg: impl prost::Message + Clone) -> Result<(), tungstenite::Error> {
        let users = self.users.lock().await;
        futures_util::future::join_all(users.iter().map(|(_, u)| u.tx.send_message(msg.clone())))
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
        available_ids: tokio::sync::Mutex::new(std::collections::VecDeque::from({
            let mut available_ids = (0..args.max_users).collect::<Vec<_>>();
            available_ids.shuffle(&mut rand::thread_rng());
            available_ids
        })),
        users: tokio::sync::Mutex::new(std::collections::HashMap::new()),
    });

    let real_ip_getter = httputil::RealIPGetter::new(args.use_x_real_ip);

    let service = routerify::RouterService::new(
        routerify::Router::builder()
            .data(real_ip_getter)
            .data(server_state)
            .get("/", handle_request)
            .build()
            .unwrap(),
    )
    .unwrap();

    let server = hyper::Server::bind(&args.bind_addr).serve(service);
    log::info!("listening on: {}", server.local_addr());
    server.await?;

    Ok(())
}
