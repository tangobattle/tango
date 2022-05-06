#![windows_subsystem = "windows"]

fn main() -> Result<(), anyhow::Error> {
    env_logger::Builder::from_default_env()
        .filter(Some("tango_core"), log::LevelFilter::Info)
        .init();

    log::info!(
        "welcome to tango-core v{}-{}!",
        env!("CARGO_PKG_VERSION"),
        git_version::git_version!()
    );

    let args = tango_core::ipc::Args::parse(
        &std::env::args()
            .nth(1)
            .ok_or_else(|| anyhow::anyhow!("missing startup args"))?,
    )?;

    mgba::log::init();

    let match_settings = args
        .match_settings
        .map(|s| {
            Ok::<_, anyhow::Error>(tango_core::battle::Settings {
                signaling_connect_addr: s.signaling_connect_addr,
                session_id: s.session_id,
                replay_metadata: s.replay_metadata.into(),
                replays_path: s.replays_path.into(),
                shadow_save_path: s.shadow_save_path.into(),
                shadow_rom_path: s.shadow_rom_path.into(),
                match_type: s.match_type,
                input_delay: s.input_delay,
                ice_servers: s.ice_servers,
            })
        })
        .map_or(Ok(None), |r| r.map(Some))?;
    log::info!("parsed match settings: {:?}", match_settings);

    let g = tango_core::game::Game::new(
        tango_core::ipc::Client::new_from_stdout(),
        args.window_title,
        args.keymapping.try_into()?,
        args.rom_path.into(),
        args.save_path.into(),
        match_settings,
    )?;
    g.run()?;
    Ok(())
}
