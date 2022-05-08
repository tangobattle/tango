mod signaling;
use envconfig::Envconfig;
use routerify::ext::RequestExt;

#[derive(Envconfig)]
struct Config {
    #[envconfig(from = "LISTEN_ADDR", default = "[::]:1984")]
    pub listen_addr: String,
}

struct State {
    signaling_server: std::sync::Arc<signaling::Server>,
}

async fn handle_signaling_request(
    mut request: hyper::Request<hyper::Body>,
) -> Result<hyper::Response<hyper::Body>, anyhow::Error> {
    if !hyper_tungstenite::is_upgrade_request(&request) {
        return Ok(hyper::Response::builder()
            .status(hyper::StatusCode::BAD_REQUEST)
            .body("Bad request".into())?);
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

fn router() -> routerify::Router<hyper::Body, anyhow::Error> {
    routerify::Router::builder()
        .data(State {
            signaling_server: std::sync::Arc::new(signaling::Server::new()),
        })
        .get("/signaling", handle_signaling_request)
        .build()
        .unwrap()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let router = router();

    env_logger::Builder::from_default_env()
        .filter(Some("tango_server"), log::LevelFilter::Info)
        .init();
    log::info!("welcome to tango-server {}!", git_version::git_version!());
    let config = Config::init_from_env().unwrap();
    let addr = config.listen_addr.parse()?;
    let service = routerify::RouterService::new(router).unwrap();
    hyper::Server::bind(&addr).serve(service).await?;
    Ok(())
}
