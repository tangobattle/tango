mod matchmaking;
use envconfig::Envconfig;
use routerify::ext::RequestExt;

#[derive(Envconfig)]
struct Config {
    #[envconfig(from = "LISTEN_ADDR", default = "[::]:1984")]
    listen_addr: String,
}

struct State {
    matchmaking_server: std::sync::Arc<matchmaking::Server>,
}

async fn handle_signaling_request(
    mut request: hyper::Request<hyper::Body>,
) -> Result<hyper::Response<hyper::Body>, anyhow::Error> {
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
            .handle_stream(websocket, &session_id)
            .await
        {
            log::error!("error in websocket connection: {}", e);
        }
    });

    Ok(response)
}

fn router() -> routerify::Router<hyper::Body, anyhow::Error> {
    routerify::Router::builder()
        .data(State {
            matchmaking_server: std::sync::Arc::new(matchmaking::Server::new()),
        })
        .get("/", handle_signaling_request)
        .get("/matchmaking", handle_signaling_request)
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
    let router = router();
    let service = routerify::RouterService::new(router).unwrap();
    hyper::Server::bind(&addr).serve(service).await?;
    Ok(())
}
