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

    /// Export to video.
    Export {
        #[clap(default_value = "ffmpeg", long)]
        ffmpeg: std::path::PathBuf,

        #[clap(default_value = "-c:a aac -ar 48000 -b:a 384k -ac 2", long)]
        ffmpeg_audio_flags: String,

        #[clap(
            default_value = "-c:v libx264 -vf scale=iw*5:ih*5:flags=neighbor,format=yuv420p -force_key_frames expr:gte(t,n_forced/2) -crf 18 -bf 2",
            long
        )]
        ffmpeg_video_flags: String,

        #[clap(default_value = "-movflags +faststart -strict -2", long)]
        ffmpeg_mux_flags: String,

        #[clap(default_value = "false", long)]
        disable_bgm: bool,

        rom_path: std::path::PathBuf,
        output_path: std::path::PathBuf,
    },
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
        Command::Export {
            ffmpeg,
            ffmpeg_audio_flags,
            ffmpeg_video_flags,
            ffmpeg_mux_flags,
            disable_bgm,
            rom_path,
            output_path,
        } => cmd_export(
            replay,
            ffmpeg,
            ffmpeg_audio_flags,
            ffmpeg_video_flags,
            ffmpeg_mux_flags,
            disable_bgm,
            rom_path,
            output_path,
        ),
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

fn cmd_export(
    replay: tango_pvp::replay::Replay,
    ffmpeg: std::path::PathBuf,
    ffmpeg_audio_flags: String,
    ffmpeg_video_flags: String,
    ffmpeg_mux_flags: String,
    disable_bgm: bool,
    rom_path: std::path::PathBuf,
    output_path: std::path::PathBuf,
) -> Result<(), anyhow::Error> {
    Ok(())
}

fn cmd_twosided_export(
    replay: tango_pvp::replay::Replay,
    ffmpeg: std::path::PathBuf,
    ffmpeg_audio_flags: String,
    ffmpeg_video_flags: String,
    ffmpeg_mux_flags: String,
    disable_bgm: bool,
    local_rom_path: std::path::PathBuf,
    remote_rom_path: std::path::PathBuf,
    output_path: std::path::PathBuf,
) -> Result<(), anyhow::Error> {
    Ok(())
}
