use clap::Parser;
use std::io::Write;

#[derive(clap::Parser)]
struct Args {
    /// Path to replay.
    path: std::path::PathBuf,

    #[command(subcommand)]
    command: Command,
}

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

pub fn main() -> Result<(), anyhow::Error> {
    let args = Args::parse();

    let mut f = std::fs::File::open(&args.path)?;
    let replay = tango_pvp::replay::Replay::decode(&mut f)?;

    match args.command {
        Command::Invert { output_path } => cmd_invert(replay, output_path),
        Command::Metadata => cmd_metadata(replay),
        Command::Wram => cmd_wram(replay),
        Command::Text => cmd_text(replay),
    }
}

fn cmd_invert(replay: tango_pvp::replay::Replay, output_path: std::path::PathBuf) -> Result<(), anyhow::Error> {
    let replay = replay.into_remote();
    let mut writer = tango_pvp::replay::Writer::new(
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

fn cmd_text(replay: tango_pvp::replay::Replay) -> Result<(), anyhow::Error> {
    for ip in &replay.input_pairs {
        println!(
            "tick = {:08x?}, l = {:02x} {:02x?}, r = {:02x} {:02x?}",
            ip.local.local_tick, ip.local.joyflags, ip.local.packet, ip.remote.joyflags, ip.remote.packet,
        );
    }
    Ok(())
}

fn cmd_metadata(replay: tango_pvp::replay::Replay) -> Result<(), anyhow::Error> {
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer_pretty(&mut stdout, &replay.metadata)?;
    stdout.write_all(b"\n")?;
    Ok(())
}

fn cmd_wram(replay: tango_pvp::replay::Replay) -> Result<(), anyhow::Error> {
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(replay.local_state.wram())?;
    Ok(())
}
