#![windows_subsystem = "windows"]

fn main() -> Result<(), Box<dyn std::error::Error>> {
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

    let g = tango_core::game::Game::new(
        tango_core::ipc::Client::new_from_stdout(),
        args.keymapping,
        args.rom_path.into(),
        args.save_path.into(),
        args.session_id,
        args.matchmaking_connect_addr,
        args.ice_servers,
        tango_core::battle::Settings {
            replay_prefix: args.replay_prefix.into(),
            match_type: args.match_type,
            input_delay: args.input_delay,
        },
    )?;
    g.run()?;
    Ok(())
}
