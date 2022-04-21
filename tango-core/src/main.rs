#![windows_subsystem = "windows"]

use clap::StructOpt;

#[derive(clap::Parser)]
struct Cli {
    #[clap(long, parse(from_os_str))]
    rom_path: std::path::PathBuf,

    #[clap(long, parse(from_os_str))]
    save_path: std::path::PathBuf,

    #[clap(long)]
    session_id: String,

    #[clap(long)]
    input_delay: u32,

    #[clap(long)]
    match_type: u16,

    #[clap(long, parse(from_os_str))]
    replay_prefix: std::path::PathBuf,

    #[clap(long)]
    matchmaking_connect_addr: String,

    #[clap(long, required = true)]
    ice_servers: Vec<String>,

    #[clap(long)]
    keymapping: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Cli::parse();

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
        serde_json::from_str(&args.keymapping)?,
        args.rom_path,
        args.save_path,
        args.session_id,
        args.matchmaking_connect_addr,
        args.ice_servers,
        tango_core::battle::Settings {
            replay_prefix: args.replay_prefix,
            match_type: args.match_type,
            input_delay: args.input_delay,
        },
    )?;
    g.run()?;
    Ok(())
}
