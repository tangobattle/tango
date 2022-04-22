#![windows_subsystem = "windows"]

fn main() -> Result<(), anyhow::Error> {
    let args = tango_core::ipc::Args::parse(
        &std::env::args()
            .nth(1)
            .ok_or_else(|| anyhow::anyhow!("missing startup args"))?,
    )?;

    env_logger::Builder::from_default_env()
        .filter(Some("tango_core"), log::LevelFilter::Info)
        .init();

    log::info!(
        "welcome to tango-core v{}-{}!",
        env!("CARGO_PKG_VERSION"),
        git_version::git_version!()
    );

    mgba::log::init();

    let match_settings = args
        .match_settings
        .map(|s| {
            Ok::<_, anyhow::Error>(tango_core::battle::Settings {
                matchmaking_connect_addr: s.matchmaking_connect_addr,
                session_id: s.session_id,
                replay_metadata: s.replay_metadata.into(),
                replay_prefix: s.replay_prefix.into(),
                match_type: s.match_type,
                input_delay: s.input_delay,
                ice_servers: s
                    .ice_servers
                    .iter()
                    .map(|url| {
                        let url = url::Url::parse(url)?;
                        Ok(webrtc::ice_transport::ice_server::RTCIceServer {
                            urls: vec![format!(
                                "{}:{}{}{}",
                                url.scheme(),
                                url.host_str()
                                    .ok_or_else(|| anyhow::anyhow!("missing host: {}", url))?,
                                url.port()
                                    .map_or_else(|| "".to_owned(), |x| format!(":{}", x)),
                                url.query()
                                    .map_or_else(|| "".to_owned(), |x| format!("?{}", x))
                            )],
                            username: url.username().to_owned(),
                            credential: url.password().unwrap_or("").to_owned(),
                            ..Default::default()
                        })
                    })
                    .collect::<Result<Vec<_>, anyhow::Error>>()?,
            })
        })
        .map_or(Ok(None), |r| r.map(Some))?;
    log::info!("parsed match settings: {:?}", match_settings);

    let g = tango_core::game::Game::new(
        tango_core::ipc::Client::new_from_stdout(),
        args.keymapping.try_into()?,
        args.rom_path.into(),
        args.save_path.into(),
        args.patch_path.map(|p| p.into()),
        match_settings,
    )?;
    g.run()?;
    Ok(())
}
