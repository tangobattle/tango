use crate::{config, replay};

#[derive(clap::Subcommand)]
pub enum Command {
    Invert { output_path: std::path::PathBuf },
    Text,
}

pub fn main(config: config::Config, path: std::path::PathBuf, command: Command) -> Result<(), anyhow::Error> {
    let mut f = std::fs::File::open(&path)?;
    let replay = replay::Replay::decode(&mut f)?;

    match command {
        Command::Invert { output_path } => cmd_invert(config, replay, output_path),
        Command::Text => cmd_text(config, replay),
    }
}

fn cmd_invert(
    _config: config::Config,
    replay: replay::Replay,
    output_path: std::path::PathBuf,
) -> Result<(), anyhow::Error> {
    let replay = replay.into_remote();
    let mut writer = replay::Writer::new(
        Box::new(std::fs::File::create(&output_path)?),
        replay.metadata,
        replay.local_player_index,
        replay.input_pairs.first().map(|ip| ip.local.packet.len()).unwrap_or(0) as u8,
    )?;
    writer.write_state(&replay.local_state)?;
    writer.write_state(&replay.remote_state)?;
    for ip in replay.input_pairs {
        writer.write_input(replay.local_player_index, &ip)?;
    }
    writer.finish()?;
    Ok(())
}

fn cmd_text(_config: config::Config, replay: replay::Replay) -> Result<(), anyhow::Error> {
    for ip in &replay.input_pairs {
        println!(
            "tick = {:08x?}, l = {:02x} {:02x?}, r = {:02x} {:02x?}",
            ip.local.local_tick, ip.local.joyflags, ip.local.packet, ip.remote.joyflags, ip.remote.packet,
        );
    }
    Ok(())
}
