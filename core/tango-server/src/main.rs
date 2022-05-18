mod httputil;
mod relay;
mod signaling;
use envconfig::Envconfig;
use prost::Message;
use routerify::ext::RequestExt;

#[derive(Envconfig)]
struct Config {
    #[envconfig(from = "LISTEN_ADDR", default = "[::]:1984")]
    listen_addr: String,

    // Don't use this unless you know what you're doing!
    #[envconfig(from = "USE_X_REAL_IP", default = "false")]
    use_x_real_ip: bool,

    #[envconfig(from = "SUBSPACE_CLIENT_ID", default = "")]
    subspace_client_id: String,

    #[envconfig(from = "SUBSPACE_CLIENT_SECRET", default = "")]
    subspace_client_secret: String,
}

struct State {
    real_ip_getter: httputil::RealIPGetter,
    relay_server: std::sync::Arc<relay::Server>,
    signaling_server: std::sync::Arc<signaling::Server>,
}

async fn handle_relay_request(
    request: hyper::Request<hyper::Body>,
) -> Result<hyper::Response<hyper::Body>, anyhow::Error> {
    let state = request.data::<State>().unwrap();
    let remote_ip = state
        .real_ip_getter
        .get_remote_real_ip(&request)
        .ok_or(anyhow::anyhow!("could not get remote ip"))?;
    let relay_server = state.relay_server.clone();
    let req =
        tango_protos::relay::GetRequest::decode(hyper::body::to_bytes(request.into_body()).await?)?;
    log::debug!("/relay: {:?}", req);
    Ok(hyper::Response::builder()
        .header(hyper::header::CONTENT_TYPE, "application/x-protobuf")
        .body(
            relay_server
                .get(remote_ip, req)
                .await?
                .encode_to_vec()
                .into(),
        )?)
}

async fn handle_signaling_request(
    mut request: hyper::Request<hyper::Body>,
) -> Result<hyper::Response<hyper::Body>, anyhow::Error> {
    if !hyper_tungstenite::is_upgrade_request(&request) {
        return Ok(hyper::Response::builder()
            .status(hyper::StatusCode::BAD_REQUEST)
            .body(
                hyper::StatusCode::BAD_REQUEST
                    .canonical_reason()
                    .unwrap()
                    .into(),
            )?);
    }

    let (response, websocket) = hyper_tungstenite::upgrade(
        &mut request,
        Some(tungstenite::protocol::WebSocketConfig {
            max_message_size: Some(4 * 1024 * 1024),
            max_frame_size: Some(1 * 1024 * 1024),
            ..Default::default()
        }),
    )?;

    let signaling_server = request.data::<State>().unwrap().signaling_server.clone();
    tokio::spawn(async move {
        let websocket = match websocket.await {
            Ok(websocket) => websocket,
            Err(e) => {
                log::error!("error in websocket connection: {}", e);
                return;
            }
        };
        if let Err(e) = signaling_server.handle_stream(websocket).await {
            log::error!("error in websocket connection: {}", e);
        }
    });

    Ok(response)
}

fn router(
    real_ip_getter: httputil::RealIPGetter,
    relay_backend: Option<Box<dyn relay::Backend + Send + Sync + 'static>>,
) -> routerify::Router<hyper::Body, anyhow::Error> {
    routerify::Router::builder()
        .data(State {
            real_ip_getter,
            relay_server: std::sync::Arc::new(relay::Server::new(relay_backend)),
            signaling_server: std::sync::Arc::new(signaling::Server::new()),
        })
        .get("/", handle_signaling_request)
        .get("/signaling", handle_signaling_request)
        .post("/relay", handle_relay_request)
        .build()
        .unwrap()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_default_env()
        .filter(Some("tango_server"), log::LevelFilter::Info)
        .init();
    log::info!("welcome to tango-server {}!", git_version::git_version!());
    let config = Config::init_from_env().unwrap();
    let real_ip_getter = httputil::RealIPGetter::new(config.use_x_real_ip);
    let relay_backend: Option<Box<dyn relay::Backend + Send + Sync + 'static>> =
        if !config.subspace_client_id.is_empty() && !config.subspace_client_secret.is_empty() {
            log::info!("using subspace relay backend");
            Some(Box::new(relay::subspace::Backend::new(
                config.subspace_client_id.clone(),
                config.subspace_client_secret.clone(),
            )))
        } else {
            log::warn!("no relay backend, will not service relay requests");
            None
        };
    let addr = config.listen_addr.parse()?;
    let router = router(real_ip_getter, relay_backend);
    let service = routerify::RouterService::new(router).unwrap();
    hyper::Server::bind(&addr).serve(service).await?;
    Ok(())
}
