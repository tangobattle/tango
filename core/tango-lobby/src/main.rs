mod signaling;
use envconfig::Envconfig;

#[derive(Envconfig)]
struct Config {
    #[envconfig(from = "LISTEN_ADDR", default = "[::]:1984")]
    pub listen_addr: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_default_env()
        .filter(Some("tango"), log::LevelFilter::Info)
        .filter(Some("signaling_server"), log::LevelFilter::Info)
        .init();
    log::info!(
        "welcome to tango's signaling_server v{}-{}!",
        env!("CARGO_PKG_VERSION"),
        git_version::git_version!()
    );
    let config = Config::init_from_env().unwrap();
    let listener = tokio::net::TcpListener::bind(config.listen_addr).await?;
    let mut server = signaling::Server::new(listener);
    server.run().await;
    Ok(())
}
