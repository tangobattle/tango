mod httputil;

use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use routerify::ext::RequestExt;

#[derive(clap::Parser)]
struct Args {
    bind_addr: std::net::SocketAddr,
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

    tokio::spawn(async move {
        let mut user_id = None;

        if let Err(e) = {
            let server_state = server_state.clone();
            let user_id = &mut user_id;
            (|| async move {
                let websocket = websocket.await?;

                let (tx, rx) = websocket.split();
                let user_state = std::sync::Arc::new(UserState {
                    tx: Sender(tokio::sync::Mutex::new(tx)),
                    ip: remote_ip,
                });

                *user_id = Some(
                    server_state
                        .next_user_id
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst),
                );

                {
                    let mut users = server_state.users.lock().await;
                    users.insert(user_id.unwrap(), user_state.clone());
                }

                handle_connection(server_state.clone(), user_state.clone(), Receiver(rx)).await?;

                Ok::<_, anyhow::Error>(())
            })()
            .await
        } {
            log::error!("connection error for {}: {}", remote_ip, e);
        }

        if let Some(user_id) = user_id {
            let mut users = server_state.users.lock().await;
            users.remove(&user_id);
        }
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
}

struct Receiver(futures_util::stream::SplitStream<hyper_tungstenite::WebSocketStream<hyper::upgrade::Upgraded>>);

impl Receiver {
    async fn recv(&mut self) -> Option<Result<tungstenite::Message, tungstenite::Error>> {
        self.0.next().await
    }

    async fn recv_binary(&mut self) -> Option<Result<Vec<u8>, tungstenite::Error>> {
        Some(self.recv().await?.and_then(|msg| {
            match msg {
                tungstenite::Message::Binary(buf) => Ok(buf),
                _ => Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("received a non-binary message: {}", msg),
                )
                .into()),
            }
        }))
    }
}

async fn handle_connection(
    server_state: std::sync::Arc<ServerState>,
    user_state: std::sync::Arc<UserState>,
    mut rx: Receiver,
) -> Result<(), anyhow::Error> {
    let mut ping_interval = tokio::time::interval(std::time::Duration::from_secs(30));

    loop {
        tokio::select! {
            msg = rx.recv() => {
                let msg = if let Some(msg) = msg {
                    msg?
                } else {
                    return Ok(());
                };
            }

            _ = ping_interval.tick() => {
            }
        }
    }
}

struct UserState {
    tx: Sender,
    ip: std::net::IpAddr,
}

struct ServerState {
    next_user_id: std::sync::atomic::AtomicUsize,
    users: tokio::sync::Mutex<std::collections::HashMap<usize, std::sync::Arc<UserState>>>,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let args = Args::parse();
    let server_state = std::sync::Arc::new(ServerState {
        next_user_id: std::sync::atomic::AtomicUsize::new(0),
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
    hyper::Server::bind(&args.bind_addr).serve(service).await?;
    Ok(())
}
