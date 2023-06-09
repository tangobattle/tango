use std::io::Write;

use crate::config;

#[derive(clap::Subcommand)]
pub enum Command {
    /// Swap sides of the replay.
    Invert { output_path: std::path::PathBuf },

    /// Dump replay metadata.
    Metadata,

    /// Dump replay WRAM.
    Wram,

    /// Dump replay in text format.
    Text,
}

pub fn main(config: config::Config, path: std::path::PathBuf, command: Command) -> Result<(), anyhow::Error> {
    let mut f = std::fs::File::open(&path)?;
    let replay = tango_replay::Replay::decode(&mut f)?;

    match command {
        Command::Invert { output_path } => cmd_invert(config, replay, output_path),
        Command::Metadata => cmd_metadata(config, replay),
        Command::Wram => cmd_wram(config, replay),
        Command::Text => cmd_text(config, replay),
    }
}

fn cmd_invert(
    _config: config::Config,
    replay: tango_replay::Replay,
    output_path: std::path::PathBuf,
) -> Result<(), anyhow::Error> {
    let replay = replay.into_remote();
    let mut writer = tango_replay::Writer::new(
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

fn cmd_text(_config: config::Config, replay: tango_replay::Replay) -> Result<(), anyhow::Error> {
    for ip in &replay.input_pairs {
        println!(
            "tick = {:08x?}, l = {:02x} {:02x?}, r = {:02x} {:02x?}",
            ip.local.local_tick, ip.local.joyflags, ip.local.packet, ip.remote.joyflags, ip.remote.packet,
        );
    }
    Ok(())
}

fn cmd_metadata(_config: config::Config, replay: tango_replay::Replay) -> Result<(), anyhow::Error> {
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer_pretty(&mut stdout, &replay.metadata)?;
    stdout.write_all(b"\n")?;
    Ok(())
}

fn cmd_wram(_config: config::Config, replay: tango_replay::Replay) -> Result<(), anyhow::Error> {
    let mut stdout = std::io::stdout().lock();
    let local_state = mgba::state::State::from_slice(&replay.local_state);
    stdout.write_all(local_state.wram())?;
    Ok(())
}
