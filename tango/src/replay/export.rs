use byteorder::ByteOrder;
use image::EncodableLayout;
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

const SAMPLE_RATE: f64 = 48000.0;

fn make_core_and_state(
    rom: &[u8],
    replay: &replay::Replay,
    settings: &Settings,
) -> anyhow::Result<(mgba::core::Core, replayer::State)> {
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

    let input_pairs = replay.input_pairs.clone();

    let replayer_state = replayer::State::new(
        (replay.metadata.match_type as u8, replay.metadata.match_subtype as u8),
        replay.local_player_index,
        input_pairs,
        0,
        Box::new(|| {}),
    );
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
    core.as_mut().load_state(&replay.local_state)?;

    Ok((core, replayer_state))
}

fn run_frame<'a>(core: &mut mgba::core::Core, samples: &'a mut [i16], emu_vbuf: &mut [u8]) -> &'a [i16] {
    core.as_mut().run_frame();

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
    video::fix_vbuf_alpha(emu_vbuf);
    samples
}

fn resolve_ffmpeg_path(ffmpeg: &Option<std::path::PathBuf>) -> std::path::PathBuf {
    ffmpeg.clone().unwrap_or_else(|| {
        let mut p = std::env::current_exe()
            .ok()
            .as_ref()
            .and_then(|p| p.parent())
            .map(|p| p.join("ffmpeg"))
            .unwrap_or("ffmpeg".into());
        p.set_extension(std::env::consts::EXE_EXTENSION);
        p
    })
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

fn make_video_ffmpeg(
    ffmpeg: &Option<std::path::PathBuf>,
    output_path: &std::path::Path,
    width: usize,
    height: usize,
    flags: &[std::ffi::OsString],
) -> anyhow::Result<tokio::process::Child> {
    let mut child = tokio::process::Command::new(resolve_ffmpeg_path(ffmpeg));
    child
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
            &format!("{}x{}", width, height),
            "-framerate",
            "16777216/280896",
            "-i",
            "pipe:",
        ])
        // Output args.
        .args(flags)
        .args(&["-f", "mp4"])
        .arg(&output_path);
    #[cfg(windows)]
    child.creation_flags(CREATE_NO_WINDOW);
    Ok(child.spawn()?)
}

fn make_audio_ffmpeg(
    ffmpeg: &Option<std::path::PathBuf>,
    output_path: &std::path::Path,
    flags: &[std::ffi::OsString],
) -> anyhow::Result<tokio::process::Child> {
    let mut child = tokio::process::Command::new(resolve_ffmpeg_path(ffmpeg));
    child
        .kill_on_drop(true)
        .stdin(std::process::Stdio::piped())
        .args(&["-y"])
        // Input args.
        .args(&["-f", "s16le", "-ar", "48k", "-ac", "2", "-i", "pipe:"])
        // Output args.
        .args(flags)
        .args(&["-f", "mp4"])
        .arg(&output_path);
    #[cfg(windows)]
    child.creation_flags(CREATE_NO_WINDOW);
    Ok(child.spawn()?)
}

fn make_mux_ffmpeg(
    ffmpeg: &Option<std::path::PathBuf>,
    output_path: &std::path::Path,
    video_input_path: &std::path::Path,
    audio_input_paths: &[&std::path::Path],
    flags: &[std::ffi::OsString],
) -> anyhow::Result<tokio::process::Child> {
    let mut child = tokio::process::Command::new(resolve_ffmpeg_path(ffmpeg));
    child
        .kill_on_drop(true)
        .args(&["-y"])
        .args(&["-i"])
        .arg(video_input_path);

    for path in audio_input_paths {
        child.args(&["-i"]).arg(path);
    }

    child.args(&["-c:v", "copy", "-c:a", "copy"]);

    child.args(&["-map", "0"]);
    for i in 0..audio_input_paths.len() {
        child.arg("-map").arg(format!("{}", i + 1));
    }

    child.args(flags);
    child.arg(&output_path);

    #[cfg(windows)]
    child.creation_flags(CREATE_NO_WINDOW);
    Ok(child.spawn()?)
}

pub async fn export(
    rom: &[u8],
    replay: &replay::Replay,
    output_path: &std::path::Path,
    settings: &Settings,
    progress_callback: impl Fn(usize, usize),
) -> anyhow::Result<()> {
    let (mut core, state) = make_core_and_state(rom, replay, settings)?;

    let filter = video::filter_by_name(&settings.video_filter).ok_or(anyhow::anyhow!("unknown filter"))?;
    let (vbuf_width, vbuf_height) =
        filter.output_size((mgba::gba::SCREEN_WIDTH as usize, mgba::gba::SCREEN_HEIGHT as usize));
    let mut emu_vbuf = vec![0u8; (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT * 4) as usize];
    let mut vbuf = vec![0u8; (vbuf_width * vbuf_height * 4) as usize];

    let video_output = tempfile::NamedTempFile::new()?;
    let mut video_child = make_video_ffmpeg(
        &settings.ffmpeg,
        video_output.path(),
        vbuf_width,
        vbuf_height,
        &shell_words::split(&settings.ffmpeg_video_flags)?
            .into_iter()
            .map(|flag| std::ffi::OsString::from(flag))
            .collect::<Vec<_>>(),
    )?;

    let audio_output = tempfile::NamedTempFile::new()?;
    let mut audio_child = make_audio_ffmpeg(
        &settings.ffmpeg,
        audio_output.path(),
        &shell_words::split(&settings.ffmpeg_audio_flags)?
            .into_iter()
            .map(|flag| std::ffi::OsString::from(flag))
            .collect::<Vec<_>>(),
    )?;

    let mut samples = vec![0i16; SAMPLE_RATE as usize];
    let total = state.lock_inner().input_pairs_left();
    loop {
        {
            let state = state.lock_inner();
            if (!replay.is_complete && state.input_pairs_left() == 0) || state.is_round_ended() {
                break;
            }
        }

        if let Some(err) = state.lock_inner().take_error() {
            Err(err)?;
        }

        let samples = run_frame(&mut core, &mut samples, &mut emu_vbuf);
        filter.apply(
            &emu_vbuf,
            &mut vbuf,
            (mgba::gba::SCREEN_WIDTH as usize, mgba::gba::SCREEN_HEIGHT as usize),
        );

        video_child.stdin.as_mut().unwrap().write_all(vbuf.as_slice()).await?;

        let mut audio_bytes = vec![0u8; samples.len() * 2];
        byteorder::LittleEndian::write_i16_into(&samples, &mut audio_bytes[..]);
        audio_child.stdin.as_mut().unwrap().write_all(&audio_bytes).await?;
        progress_callback(total - state.lock_inner().input_pairs_left(), total);
    }

    video_child.stdin = None;
    video_child.wait().await?;
    audio_child.stdin = None;
    audio_child.wait().await?;

    let mut mux_child = make_mux_ffmpeg(
        &settings.ffmpeg,
        output_path,
        video_output.path(),
        &[audio_output.path()],
        &shell_words::split(&settings.ffmpeg_mux_flags)?
            .into_iter()
            .map(|flag| std::ffi::OsString::from(flag))
            .collect::<Vec<_>>(),
    )?;
    mux_child.wait().await?;

    Ok(())
}

pub async fn export_twosided(
    local_rom: &[u8],
    remote_rom: &[u8],
    replay: &replay::Replay,
    output_path: &std::path::Path,
    settings: &Settings,
    progress_callback: impl Fn(usize, usize),
) -> anyhow::Result<()> {
    let local_replay = replay.clone();
    let remote_replay = local_replay.clone().into_remote();

    let (mut local_core, local_state) = make_core_and_state(local_rom, &local_replay, settings)?;
    let (mut remote_core, remote_state) = make_core_and_state(remote_rom, &remote_replay, settings)?;

    let mut emu_vbuf = vec![0u8; (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT * 4) as usize];

    let filter = video::filter_by_name(&settings.video_filter).ok_or(anyhow::anyhow!("unknown filter"))?;
    let (vbuf_width, vbuf_height) =
        filter.output_size((mgba::gba::SCREEN_WIDTH as usize, mgba::gba::SCREEN_HEIGHT as usize));
    let mut vbuf = image::RgbaImage::new(vbuf_width as u32, vbuf_height as u32);
    let mut composed_vbuf = image::RgbaImage::new((vbuf_width * 2) as u32, vbuf_height as u32);

    let video_output = tempfile::NamedTempFile::new()?;
    let mut video_child = make_video_ffmpeg(
        &settings.ffmpeg,
        video_output.path(),
        vbuf_width * 2,
        vbuf_height,
        &shell_words::split(&settings.ffmpeg_video_flags)?
            .into_iter()
            .map(|flag| std::ffi::OsString::from(flag))
            .collect::<Vec<_>>(),
    )?;

    let local_audio_output = tempfile::NamedTempFile::new()?;
    let mut local_audio_child = make_audio_ffmpeg(
        &settings.ffmpeg,
        local_audio_output.path(),
        &shell_words::split(&settings.ffmpeg_audio_flags)?
            .into_iter()
            .map(|flag| std::ffi::OsString::from(flag))
            .collect::<Vec<_>>(),
    )?;

    let remote_audio_output = tempfile::NamedTempFile::new()?;
    let mut remote_audio_child = make_audio_ffmpeg(
        &settings.ffmpeg,
        remote_audio_output.path(),
        &shell_words::split(&settings.ffmpeg_audio_flags)?
            .into_iter()
            .map(|flag| std::ffi::OsString::from(flag))
            .collect::<Vec<_>>(),
    )?;

    let mut samples = vec![0i16; SAMPLE_RATE as usize];
    let total = std::cmp::min(
        local_state.lock_inner().input_pairs_left(),
        remote_state.lock_inner().input_pairs_left(),
    );
    loop {
        {
            let local_state = local_state.lock_inner();
            if (!local_replay.is_complete && local_state.input_pairs_left() == 0) || local_state.is_round_ended() {
                break;
            }
        }

        {
            let remote_state = remote_state.lock_inner();
            if (!remote_replay.is_complete && remote_state.input_pairs_left() == 0) || remote_state.is_round_ended() {
                break;
            }
        }

        let current_tick = local_state.lock_inner().current_tick();
        if remote_state.lock_inner().current_tick() != current_tick {
            anyhow::bail!(
                "tick misaligned! {} vs {}",
                current_tick,
                remote_state.lock_inner().current_tick()
            );
        }

        while local_state.lock_inner().current_tick() == current_tick
            && remote_state.lock_inner().current_tick() == current_tick
        {
            if let Some(err) = local_state.lock_inner().take_error() {
                Err(err)?;
            }

            if let Some(err) = remote_state.lock_inner().take_error() {
                Err(err)?;
            }

            {
                let local_samples = run_frame(&mut local_core, &mut samples, &mut emu_vbuf);
                filter.apply(
                    &emu_vbuf,
                    &mut vbuf,
                    (mgba::gba::SCREEN_WIDTH as usize, mgba::gba::SCREEN_HEIGHT as usize),
                );
                image::imageops::replace(&mut composed_vbuf, &vbuf, 0, 0);
                let mut audio_bytes = vec![0u8; local_samples.len() * 2];
                byteorder::LittleEndian::write_i16_into(&local_samples, &mut audio_bytes[..]);
                local_audio_child
                    .stdin
                    .as_mut()
                    .unwrap()
                    .write_all(&audio_bytes)
                    .await?;
            }

            {
                let remote_samples = run_frame(&mut remote_core, &mut samples, &mut emu_vbuf);
                filter.apply(
                    &emu_vbuf,
                    &mut vbuf,
                    (mgba::gba::SCREEN_WIDTH as usize, mgba::gba::SCREEN_HEIGHT as usize),
                );
                image::imageops::replace(&mut composed_vbuf, &vbuf, vbuf_width as i64, 0);
                let mut audio_bytes = vec![0u8; remote_samples.len() * 2];
                byteorder::LittleEndian::write_i16_into(&remote_samples, &mut audio_bytes[..]);
                remote_audio_child
                    .stdin
                    .as_mut()
                    .unwrap()
                    .write_all(&audio_bytes)
                    .await?;
            }

            video_child
                .stdin
                .as_mut()
                .unwrap()
                .write_all(composed_vbuf.as_bytes())
                .await?;
        }

        while local_state.lock_inner().current_tick() == current_tick {
            run_frame(&mut local_core, &mut samples, &mut emu_vbuf);
        }

        while remote_state.lock_inner().current_tick() == current_tick {
            run_frame(&mut remote_core, &mut samples, &mut emu_vbuf);
        }

        progress_callback(current_tick as usize, total);
    }

    video_child.stdin = None;
    video_child.wait().await?;
    local_audio_child.stdin = None;
    local_audio_child.wait().await?;
    remote_audio_child.stdin = None;
    remote_audio_child.wait().await?;

    let mut mux_child = make_mux_ffmpeg(
        &settings.ffmpeg,
        output_path,
        video_output.path(),
        &[local_audio_output.path(), remote_audio_output.path()],
        &shell_words::split(&settings.ffmpeg_mux_flags)?
            .into_iter()
            .map(|flag| std::ffi::OsString::from(flag))
            .collect::<Vec<_>>(),
    )?;
    mux_child.wait().await?;

    Ok(())
}
