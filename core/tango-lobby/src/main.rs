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
            .body(hyper::Body::empty())?);
    }

    let (response, websocket) = hyper_tungstenite::upgrade(&mut request, None)?;

    // Spawn a task to handle the websocket connection.
    let signaling_server = request.data::<State>().unwrap().signaling_server.clone();
    tokio::spawn(async move {
        if let Err(e) = signaling_server.handle_connection(websocket).await {
            eprintln!("Error in websocket connection: {}", e);
        }
    });

    // Return the response so the spawned future can continue.
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
        .filter(Some("tango_lobby"), log::LevelFilter::Info)
        .init();
    log::info!("welcome to tango-lobby {}!", git_version::git_version!());
    let config = Config::init_from_env().unwrap();
    let addr = config.listen_addr.parse()?;
    let service = routerify::RouterService::new(router).unwrap();
    hyper::Server::bind(&addr).serve(service).await?;
    Ok(())
}
