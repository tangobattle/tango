use clap::Parser;
use std::io::Write;

#[derive(clap::Parser)]
struct Args {
    /// Path to replay.
    path: std::path::PathBuf,

    /// Invert the replay?
    #[clap(default_value = "true", long)]
    invert: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
pub enum Command {
    /// Copy the replay.
    Copy { output_path: std::path::PathBuf },

    /// Dump replay metadata.
    Metadata,

    /// Dump replay local-side SRAM.
    Sram,

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

        /// Render both sides of the match side-by-side. Without this,
        /// only the local-perspective core is rendered.
        #[clap(long)]
        twosided: bool,

        local_rom_path: std::path::PathBuf,

        /// Required even for one-sided rendering: shadow plays the opponent's
        /// ROM from the recorded remote joyflags to re-derive the per-tick
        /// remote packets that the local-side core needs.
        #[clap(long)]
        remote_rom_path: Option<std::path::PathBuf>,

        output_path: std::path::PathBuf,
    },

    /// Evaluate the result of a replay.
    Eval { rom_path: std::path::PathBuf },
}

#[tokio::main]
pub async fn main() -> Result<(), anyhow::Error> {
    let args = Args::parse();

    let mut f = std::fs::File::open(&args.path)?;
    let mut replay = tango_pvp::replay::Replay::decode(&mut f)?;

    if args.invert {
        replay = replay.into_remote();
    }

    match args.command {
        Command::Copy { output_path } => cmd_copy(replay, output_path).await,
        Command::Metadata => cmd_metadata(replay).await,
        Command::Sram => cmd_sram(replay).await,
        Command::Text => cmd_text(replay).await,
        Command::Export {
            ffmpeg,
            ffmpeg_audio_flags,
            ffmpeg_video_flags,
            ffmpeg_mux_flags,
            disable_bgm,
            twosided,
            local_rom_path,
            remote_rom_path,
            output_path,
        } => {
            cmd_export(
                replay,
                ffmpeg,
                ffmpeg_audio_flags,
                ffmpeg_video_flags,
                ffmpeg_mux_flags,
                disable_bgm,
                twosided,
                local_rom_path,
                remote_rom_path,
                output_path,
            )
            .await
        }
        Command::Eval { rom_path } => cmd_eval(replay, rom_path).await,
    }
}

async fn cmd_copy(replay: tango_pvp::replay::Replay, output_path: std::path::PathBuf) -> Result<(), anyhow::Error> {
    let mut writer = tango_pvp::replay::Writer::new(
        Box::new(std::fs::File::create(output_path)?),
        replay.metadata,
        replay.is_offerer,
        replay.local_player_index,
        replay.rng_seed,
        &replay.local_sram,
        &replay.remote_sram,
    )?;
    for round in &replay.rounds {
        writer.start_round()?;
        for ip in round {
            writer.write_input(replay.local_player_index, ip)?;
        }
    }
    writer.finish()?;
    Ok(())
}

async fn cmd_text(replay: tango_pvp::replay::Replay) -> Result<(), anyhow::Error> {
    for (idx, round) in replay.rounds.iter().enumerate() {
        println!("=== round {} ({} inputs) ===", idx + 1, round.len());
        for (tick, ip) in round.iter().enumerate() {
            println!(
                "tick = {:08x?}, l = {:04x}, r = {:04x}",
                tick, ip.local.joyflags, ip.remote.joyflags,
            );
        }
    }
    Ok(())
}

async fn cmd_metadata(replay: tango_pvp::replay::Replay) -> Result<(), anyhow::Error> {
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer_pretty(&mut stdout, &replay.metadata)?;
    stdout.write_all(b"\n")?;
    Ok(())
}

async fn cmd_sram(replay: tango_pvp::replay::Replay) -> Result<(), anyhow::Error> {
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(&replay.local_sram)?;
    Ok(())
}

async fn cmd_export(
    replay: tango_pvp::replay::Replay,
    ffmpeg: std::path::PathBuf,
    ffmpeg_audio_flags: String,
    ffmpeg_video_flags: String,
    ffmpeg_mux_flags: String,
    disable_bgm: bool,
    twosided: bool,
    local_rom_path: std::path::PathBuf,
    remote_rom_path: Option<std::path::PathBuf>,
    output_path: std::path::PathBuf,
) -> Result<(), anyhow::Error> {
    let bar: indicatif::ProgressBar = indicatif::ProgressBar::new(0);
    let cb = move |current, total| {
        bar.set_length(total as u64);
        bar.set_position(current as u64);
    };

    let settings = tango_pvp::replay::export::Settings {
        ffmpeg: Some(ffmpeg),
        ffmpeg_audio_flags,
        ffmpeg_video_flags,
        ffmpeg_mux_flags,
        disable_bgm,
    };

    let local_rom = std::fs::read(&local_rom_path)?;
    let local_detected_game = tango_gamedb::detect(&local_rom).ok_or(anyhow::anyhow!("rom detection failed"))?;
    let local_game_info = replay
        .metadata
        .local_side
        .as_ref()
        .and_then(|side| side.game_info.as_ref())
        .ok_or(anyhow::anyhow!("missing local game info"))?;
    let local_game =
        tango_gamedb::find_by_family_and_variant(&local_game_info.rom_family, local_game_info.rom_variant as u8)
            .unwrap();
    let local_hooks = tango_pvp::hooks::hooks_for_gamedb_entry(local_game).unwrap();
    if local_game != local_detected_game {
        return Err(anyhow::format_err!(
            "expected game {:?}, got {:?}",
            local_game.family_and_variant(),
            local_detected_game.family_and_variant()
        ));
    }

    // Shadow always needs the remote ROM, even on a one-sided export —
    // running it against the local ROM for a cross-game replay would
    // produce nonsense packets. Require remote_rom_path.
    let remote_rom_path =
        remote_rom_path.ok_or_else(|| anyhow::anyhow!("remote_rom_path is required (shadow needs the remote rom)"))?;
    let remote_rom = std::fs::read(&remote_rom_path)?;
    let remote_detected_game = tango_gamedb::detect(&remote_rom).ok_or(anyhow::anyhow!("rom detection failed"))?;
    let remote_game_info = replay
        .metadata
        .remote_side
        .as_ref()
        .and_then(|side| side.game_info.as_ref())
        .ok_or(anyhow::anyhow!("missing remote game info"))?;
    let remote_game =
        tango_gamedb::find_by_family_and_variant(&remote_game_info.rom_family, remote_game_info.rom_variant as u8)
            .unwrap();
    let remote_hooks = tango_pvp::hooks::hooks_for_gamedb_entry(remote_game).unwrap();
    if remote_game != remote_detected_game {
        return Err(anyhow::format_err!(
            "expected game {:?}, got {:?}",
            remote_game.family_and_variant(),
            remote_detected_game.family_and_variant()
        ));
    }

    let selected_rounds = vec![vec![true; replay.rounds.len()]];
    let canceller = tango_pvp::replay::export::Canceller::new();
    if twosided {
        tango_pvp::replay::export::export_twosided(
            &local_rom,
            local_hooks,
            &remote_rom,
            remote_hooks,
            &[replay],
            &selected_rounds,
            &output_path,
            &settings,
            &canceller,
            cb,
        )?;
    } else {
        tango_pvp::replay::export::export(
            &local_rom,
            local_hooks,
            &remote_rom,
            remote_hooks,
            &[replay],
            &selected_rounds,
            &output_path,
            &settings,
            &canceller,
            cb,
        )?;
    }

    Ok(())
}

async fn cmd_eval(replay: tango_pvp::replay::Replay, rom_path: std::path::PathBuf) -> Result<(), anyhow::Error> {
    let rom = std::fs::read(&rom_path)?;
    let detected_game = tango_gamedb::detect(&rom).ok_or(anyhow::anyhow!("rom detection failed"))?;
    let game_info = replay
        .metadata
        .local_side
        .as_ref()
        .and_then(|side| side.game_info.as_ref())
        .ok_or(anyhow::anyhow!("missing local game info"))?;
    let game = tango_gamedb::find_by_family_and_variant(&game_info.rom_family, game_info.rom_variant as u8).unwrap();
    let hooks = tango_pvp::hooks::hooks_for_gamedb_entry(game).unwrap();
    if game != detected_game {
        return Err(anyhow::format_err!(
            "expected game {:?}, got {:?}",
            game.family_and_variant(),
            detected_game.family_and_variant()
        ));
    }

    let (result, _) = tango_pvp::eval::eval(&replay, &rom, hooks, Vec::new).await?;
    println!("{}", result.outcome as u8);

    Ok(())
}
