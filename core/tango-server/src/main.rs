mod httputil;
mod relay;
mod signaling;
use envconfig::Envconfig;
use prost::Message;
use routerify::ext::RequestExt;

#[derive(Envconfig)]
struct Config {
    #[envconfig(from = "LISTEN_ADDR", default = "[::]:1984")]
    pub listen_addr: String,

    // Don't use this unless you know what you're doing!
    #[envconfig(from = "USE_X_REAL_IP", default = "false")]
    pub use_x_real_ip: bool,
}

struct State {
    real_ip_getter: httputil::RealIPGetter,
    relay_server: Option<std::sync::Arc<relay::Server>>,
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
    let relay_server = if let Some(relay_server) = state.relay_server.as_ref() {
        relay_server.clone()
    } else {
        return Ok(hyper::Response::builder()
            .status(hyper::StatusCode::NOT_IMPLEMENTED)
            .body(hyper::StatusCode::NOT_IMPLEMENTED.as_str().into())?);
    };
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
            .body(hyper::StatusCode::BAD_REQUEST.as_str().into())?);
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

fn router(config: &Config) -> routerify::Router<hyper::Body, anyhow::Error> {
    routerify::Router::builder()
        .data(State {
            real_ip_getter: httputil::RealIPGetter::new(config.use_x_real_ip),
            relay_server: Some(std::sync::Arc::new(relay::Server::new())),
            signaling_server: std::sync::Arc::new(signaling::Server::new()),
        })
        .get("/", handle_signaling_request)
        .get("/signaling", handle_signaling_request)
        .get("/relay", handle_relay_request)
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
    let addr = config.listen_addr.parse()?;
    let router = router(&config);
    let service = routerify::RouterService::new(router).unwrap();
    hyper::Server::bind(&addr).serve(service).await?;
    Ok(())
}
