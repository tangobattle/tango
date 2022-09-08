use byteorder::ByteOrder;
use tokio::io::AsyncWriteExt;

use crate::{game, replay, replayer, video};

pub struct Settings {
    pub ffmpeg: Option<std::path::PathBuf>,
    pub ffmpeg_audio_flags: String,
    pub ffmpeg_video_flags: String,
    pub ffmpeg_mux_flags: String,
    pub video_filter: String,
    pub disable_bgm: bool,
}

impl Settings {
    pub fn default_with_scale(factor: usize) -> Self {
        Self {
            ffmpeg: None,
            ffmpeg_audio_flags: "-c:a aac -ar 48000 -b:a 384k -ac 2".to_string(),
            ffmpeg_video_flags: format!("-c:v libx264 -vf scale=iw*{}:ih*{}:flags=neighbor,format=yuv420p -force_key_frames expr:gte(t,n_forced/2) -crf 18 -bf 2", factor, factor),
            ffmpeg_mux_flags: "-movflags +faststart".to_string(),
            video_filter: "".to_string(),
            disable_bgm: false,
        }
    }
}

pub async fn export(
    rom: &[u8],
    replay: &replay::Replay,
    output_path: &std::path::Path,
    settings: &Settings,
    progress_callback: impl Fn(usize, usize),
) -> anyhow::Result<()> {
    let ffmpeg = settings.ffmpeg.clone().unwrap_or_else(|| {
        let mut p = std::env::current_exe()
            .ok()
            .as_ref()
            .and_then(|p| p.parent())
            .map(|p| p.join("ffmpeg"))
            .unwrap_or("ffmpeg".into());
        p.set_extension(std::env::consts::EXE_EXTENSION);
        p
    });

    let mut core = mgba::core::Core::new_gba("tango")?;
    core.enable_video_buffer();

    core.as_mut().load_rom(mgba::vfile::VFile::open_memory(&rom))?;
    core.as_mut().reset();

    let game_info = replay
        .metadata
        .local_side
        .as_ref()
        .and_then(|side| side.game_info.as_ref())
        .ok_or(anyhow::anyhow!("missing game info"))?;

    let local_state = replay
        .local_state
        .as_ref()
        .ok_or(anyhow::anyhow!("missing local state"))?;

    let input_pairs = replay.input_pairs.clone();

    let replayer_state = replayer::State::new(replay.local_player_index, input_pairs, 0, Box::new(|| {}));
    replayer_state.lock_inner().set_disable_bgm(settings.disable_bgm);
    let game = game::find_by_family_and_variant(&game_info.rom_family, game_info.rom_variant as u8)
        .ok_or(anyhow::anyhow!("game not found"))?;

    let hooks = game.hooks();
    hooks.patch(core.as_mut());
    {
        let replayer_state = replayer_state.clone();
        let mut traps = hooks.common_traps();
        traps.extend(hooks.replayer_traps(replayer_state.clone()));
        core.set_traps(traps);
    }
    core.as_mut().load_state(&local_state)?;

    #[cfg(windows)]
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let filter = video::filter_by_name(&settings.video_filter).ok_or(anyhow::anyhow!("unknown filter"))?;
    let (vbuf_width, vbuf_height) =
        filter.output_size((mgba::gba::SCREEN_WIDTH as usize, mgba::gba::SCREEN_HEIGHT as usize));
    let mut emu_vbuf = vec![0u8; (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT * 4) as usize];
    let mut vbuf = vec![0u8; (vbuf_width * vbuf_height * 4) as usize];

    let video_output = tempfile::NamedTempFile::new()?;
    let mut video_child = tokio::process::Command::new(&ffmpeg);
    video_child
        .kill_on_drop(true)
        .stdin(std::process::Stdio::piped())
        .args(&["-y"])
        // Input args.
        .args(&[
            "-f",
            "rawvideo",
            "-pixel_format",
            "rgba",
            "-video_size",
            &format!("{}x{}", vbuf_width, vbuf_height),
            "-framerate",
            "16777216/280896",
            "-i",
            "pipe:",
        ])
        // Output args.
        .args(shell_words::split(&settings.ffmpeg_video_flags)?)
        .args(&["-f", "mp4"])
        .arg(&video_output.path());
    #[cfg(windows)]
    video_child.creation_flags(CREATE_NO_WINDOW);
    let mut video_child = video_child.spawn()?;

    let audio_output = tempfile::NamedTempFile::new()?;
    let mut audio_child = tokio::process::Command::new(&ffmpeg);
    audio_child
        .kill_on_drop(true)
        .stdin(std::process::Stdio::piped())
        .args(&["-y"])
        // Input args.
        .args(&["-f", "s16le", "-ar", "48k", "-ac", "2", "-i", "pipe:"])
        // Output args.
        .args(shell_words::split(&settings.ffmpeg_audio_flags)?)
        .args(&["-f", "mp4"])
        .arg(&audio_output.path());
    #[cfg(windows)]
    audio_child.creation_flags(CREATE_NO_WINDOW);
    let mut audio_child = audio_child.spawn()?;

    const SAMPLE_RATE: f64 = 48000.0;
    let mut samples = vec![0i16; SAMPLE_RATE as usize];
    let total = replayer_state.lock_inner().input_pairs_left();
    loop {
        {
            let replayer_state = replayer_state.lock_inner();
            if (!replay.is_complete && replayer_state.input_pairs_left() == 0) || replayer_state.is_round_ended() {
                break;
            }
        }

        core.as_mut().run_frame();

        if let Some(err) = replayer_state.lock_inner().take_error() {
            Err(err)?;
        }

        let clock_rate = core.as_ref().frequency();
        let n = {
            let mut core = core.as_mut();
            let mut left = core.audio_channel(0);
            left.set_rates(clock_rate as f64, SAMPLE_RATE);
            let n = left.samples_avail();
            left.read_samples(&mut samples[..(n * 2) as usize], left.samples_avail(), true);
            n
        };
        {
            let mut core = core.as_mut();
            let mut right = core.audio_channel(1);
            right.set_rates(clock_rate as f64, SAMPLE_RATE);
            right.read_samples(&mut samples[1..(n * 2) as usize], n, true);
        }
        let samples = &samples[..(n * 2) as usize];

        emu_vbuf.copy_from_slice(core.video_buffer().unwrap());
        video::fix_vbuf_alpha(&mut emu_vbuf);
        filter.apply(
            &emu_vbuf,
            &mut vbuf,
            (mgba::gba::SCREEN_WIDTH as usize, mgba::gba::SCREEN_HEIGHT as usize),
        );

        video_child.stdin.as_mut().unwrap().write_all(vbuf.as_slice()).await?;

        let mut audio_bytes = vec![0u8; samples.len() * 2];
        byteorder::LittleEndian::write_i16_into(samples, &mut audio_bytes[..]);
        audio_child.stdin.as_mut().unwrap().write_all(&audio_bytes).await?;
        progress_callback(total - replayer_state.lock_inner().input_pairs_left(), total);
    }

    video_child.stdin = None;
    video_child.wait().await?;
    audio_child.stdin = None;
    audio_child.wait().await?;

    let mut mux_child = tokio::process::Command::new(&ffmpeg);
    mux_child
        .kill_on_drop(true)
        .args(&["-y"])
        .args(&["-i"])
        .arg(video_output.path())
        .args(&["-i"])
        .arg(audio_output.path())
        .args(&["-c:v", "copy", "-c:a", "copy"])
        .args(shell_words::split(&settings.ffmpeg_mux_flags)?)
        .arg(&output_path);
    #[cfg(windows)]
    mux_child.creation_flags(CREATE_NO_WINDOW);
    let mut mux_child = mux_child.spawn()?;
    mux_child.wait().await?;

    Ok(())
}
