mod httputil;
mod iceconfig;
mod matchmaking;
use envconfig::Envconfig;
use routerify::ext::RequestExt;

#[derive(Envconfig)]
struct Config {
    #[envconfig(from = "LISTEN_ADDR", default = "[::]:1984")]
    listen_addr: String,

    // Don't use this unless you know what you're doing!
    #[envconfig(from = "USE_X_REAL_IP", default = "false")]
    use_x_real_ip: bool,

    #[envconfig(from = "TWILIO_ACCOUNT_SID", default = "")]
    twilio_account_sid: String,

    #[envconfig(from = "TWILIO_API_SID", default = "")]
    twilio_api_sid: String,

    #[envconfig(from = "TWILIO_API_SECRET", default = "")]
    twilio_api_secret: String,

    #[envconfig(from = "OPENTOK_API_KEY", default = "")]
    opentok_api_key: String,

    #[envconfig(from = "OPENTOK_API_SECRET", default = "")]
    opentok_api_secret: String,

    #[envconfig(from = "METERED_USERNAME", default = "")]
    metered_username: String,

    #[envconfig(from = "METERED_CREDENTIAL", default = "")]
    metered_credential: String,
}

struct State {
    real_ip_getter: httputil::RealIPGetter,
    matchmaking_server: std::sync::Arc<matchmaking::Server>,
}

async fn handle_healthcheck_request(
    _request: hyper::Request<hyper::Body>,
) -> Result<hyper::Response<hyper::Body>, anyhow::Error> {
    return Ok(hyper::Response::builder()
        .status(hyper::StatusCode::OK)
        .body(hyper::Body::from("ok"))
        .unwrap());
}

async fn handle_matchmaking_request(
    mut request: hyper::Request<hyper::Body>,
) -> Result<hyper::Response<hyper::Body>, anyhow::Error> {
    let remote_ip = if let Some(remote_ip) = request
        .data::<State>()
        .unwrap()
        .real_ip_getter
        .get_remote_real_ip(&request)
    {
        remote_ip
    } else {
        return Ok(hyper::Response::builder()
            .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
            .body(hyper::Body::from("internal error"))
            .unwrap());
    };

    let session_id = if let Some(session_id) = request.uri().query().and_then(|query| {
        url::form_urlencoded::parse(query.as_bytes())
            .into_owned()
            .find(|(k, _)| k == "session_id")
            .map(|(_, v)| v)
    }) {
        session_id
    } else {
        return Ok(hyper::Response::builder()
            .status(hyper::StatusCode::BAD_REQUEST)
            .body(hyper::Body::from("missing session_id"))
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

    let matchmaking_server = request.data::<State>().unwrap().matchmaking_server.clone();
    tokio::spawn(async move {
        let websocket = match websocket.await {
            Ok(websocket) => websocket,
            Err(e) => {
                log::error!("error in websocket connection: {}", e);
                return;
            }
        };

        if let Err(e) = matchmaking_server
            .handle_stream(websocket, remote_ip, &session_id)
            .await
        {
            log::error!("error in websocket connection: {}", e);
        }
    });

    Ok(response)
}

fn router(
    real_ip_getter: httputil::RealIPGetter,
    iceconfig_backend: Option<Box<dyn iceconfig::Backend + Send + Sync + 'static>>,
) -> routerify::Router<hyper::Body, anyhow::Error> {
    routerify::Router::builder()
        .data(State {
            real_ip_getter,
            matchmaking_server: std::sync::Arc::new(matchmaking::Server::new(iceconfig_backend)),
        })
        .get("/", handle_matchmaking_request)
        .get("/ok", handle_healthcheck_request)
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
    let addr = config.listen_addr.parse()?;

    let iceconfig_backend: Option<Box<dyn iceconfig::Backend + Send + Sync + 'static>> =
        if !config.twilio_account_sid.is_empty()
            && !config.twilio_api_sid.is_empty()
            && !config.twilio_api_secret.is_empty()
        {
            log::info!("using twilio iceconfig backend");
            Some(Box::new(iceconfig::twilio::Backend::new(
                config.twilio_account_sid.clone(),
                config.twilio_api_sid.clone(),
                config.twilio_api_secret.clone(),
            )))
        } else if !config.opentok_api_key.is_empty() && !config.opentok_api_secret.is_empty() {
            log::info!("using opentok iceconfig backend");
            Some(Box::new(iceconfig::opentok::Backend::new(
                config.opentok_api_key.clone(),
                config.opentok_api_secret.clone(),
            )))
        } else if !config.metered_username.is_empty() && !config.metered_credential.is_empty() {
            log::info!("using opentok iceconfig backend");
            Some(Box::new(iceconfig::metered::Backend::new(
                config.metered_username.clone(),
                config.metered_credential.clone(),
            )))
        } else {
            log::warn!("no iceconfig backend, will not service iceconfig requests");
            None
        };

    let router = router(real_ip_getter, iceconfig_backend);

    let service = routerify::RouterService::new(router).unwrap();
    hyper::Server::bind(&addr).serve(service).await?;
    Ok(())
}
