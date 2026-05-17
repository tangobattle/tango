use byteorder::ByteOrder;
use image::EncodableLayout;
use tokio::io::AsyncWriteExt;

pub struct Settings {
    pub ffmpeg: Option<std::path::PathBuf>,
    pub ffmpeg_audio_flags: String,
    pub ffmpeg_video_flags: String,
    pub ffmpeg_mux_flags: String,
    pub disable_bgm: bool,
}

impl Settings {
    pub fn default_with_scale(factor: Option<usize>) -> Self {
        Self {
            ffmpeg: None,
            ffmpeg_audio_flags: if factor.is_some() {
                "-c:a aac -ar 48000 -b:a 384k -ac 2".to_string()
            } else {
                "-c:a flac".to_string()
            },
            ffmpeg_video_flags: if let Some(factor) = factor {
                format!("-c:v libx264 -vf scale=iw*{}:ih*{}:flags=neighbor,format=yuv420p -force_key_frames expr:gte(t,n_forced/2) -crf 18 -bf 2", factor, factor)
            } else {
                "-c:v libx264rgb -preset ultrafast -qp 0".to_string()
            },
            ffmpeg_mux_flags: "-movflags +faststart -strict -2".to_string(),
            disable_bgm: false,
        }
    }
}

fn fix_vbuf_alpha(vbuf: &mut [u8]) {
    for chunk in vbuf.chunks_mut(4) {
        chunk[3] = 0xff;
    }
}

const SAMPLE_RATE: f64 = 48000.0;

fn make_core_and_state(
    rom: &[u8],
    sram: &[u8],
    hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
    shadow_rom: &[u8],
    shadow_hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
    replay: &crate::replay::Replay,
    settings: &Settings,
) -> anyhow::Result<(mgba::core::Core, crate::stepper::State)> {
    let mut core = mgba::core::Core::new_gba("tango")?;
    core.enable_video_buffer();

    core.as_mut().load_rom(mgba::vfile::VFile::from_vec(rom.to_vec()))?;
    core.as_mut().load_save(mgba::vfile::VFile::from_vec(sram.to_vec()))?;
    core.as_mut().reset();

    if replay.rounds.is_empty() {
        return Err(anyhow::anyhow!("replay has no rounds"));
    }

    let total_replay_ticks = replay.rounds.iter().map(|r| r.len() as u32).sum::<u32>();
    let match_type = (replay.metadata.match_type as u8, replay.metadata.match_subtype as u8);

    let shadow = crate::shadow::Shadow::new_for_replay(shadow_rom, replay, shadow_hooks)?;
    let shadow = std::sync::Arc::new(parking_lot::Mutex::new(shadow));

    let stepper_state = crate::stepper::State::new(
        match_type,
        replay.local_player_index,
        replay.rounds.clone(),
        0,
        replay.rng_seed,
        replay.is_offerer,
        total_replay_ticks,
        shadow,
        Box::new(|| {}),
    );
    stepper_state.lock_inner().set_disable_bgm(settings.disable_bgm);

    hooks.patch(core.as_mut());
    {
        let stepper_state = stepper_state.clone();
        let mut traps = hooks.common_traps();
        traps.extend(hooks.stepper_traps(stepper_state.clone()));
        core.set_traps(traps);
    }

    Ok((core, stepper_state))
}

const AUDIO_CHANNELS: usize = 2;

fn run_frame<'a>(
    core: &mut mgba::core::Core,
    resampler: &mut mgba::audio::AudioResampler,
    dest_buffer: &mut mgba::audio::AudioBuffer,
    samples: &'a mut [i16],
    emu_vbuf: &mut [u8],
) -> &'a [i16] {
    core.as_mut().run_frame();

    let n = {
        let mut core = core.as_mut();
        let core_rate = core.as_ref().audio_sample_rate() as f64;
        let core_buffer_ptr = core.audio_buffer().as_mut_ptr();
        resampler.set_source(core_buffer_ptr, core_rate, true);
        resampler.set_destination(dest_buffer.as_mut_ptr(), SAMPLE_RATE);
        resampler.process();
        let cap = samples.len() / AUDIO_CHANNELS;
        let frames = dest_buffer.available().min(cap);
        dest_buffer.read(&mut samples[..frames * AUDIO_CHANNELS], frames);
        frames
    };

    let samples = &samples[..n * AUDIO_CHANNELS];

    emu_vbuf.copy_from_slice(core.video_buffer().unwrap());
    fix_vbuf_alpha(emu_vbuf);
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

        if p.exists() {
            p
        } else {
            "ffmpeg".into()
        }
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
        .args(["-y"])
        // Input args.
        .args([
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
        .args(["-f", "matroska"])
        .arg(output_path);
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
        .args(["-y"])
        // Input args.
        .args(["-f", "s16le", "-ar", "48k", "-ac", "2", "-i", "pipe:"])
        // Output args.
        .args(flags)
        .args(["-f", "matroska"])
        .arg(output_path);
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
    child.kill_on_drop(true).args(["-y"]).args(["-i"]).arg(video_input_path);

    for path in audio_input_paths {
        child.args(["-i"]).arg(path);
    }

    child.args(["-c:v", "copy", "-c:a", "copy"]);

    child.args(["-map", "0"]);
    for i in 0..audio_input_paths.len() {
        child.arg("-map").arg(format!("{}", i + 1));
    }

    child.args(flags);
    child.arg(output_path);

    #[cfg(windows)]
    child.creation_flags(CREATE_NO_WINDOW);
    Ok(child.spawn()?)
}

fn kept_input_pairs(replay: &crate::replay::Replay, selected: &[bool]) -> usize {
    match selected.iter().rposition(|&s| s) {
        Some(last) => replay.rounds.iter().take(last + 1).map(|r| r.len()).sum(),
        None => 0,
    }
}

pub async fn export(
    local_rom: &[u8],
    local_hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
    remote_rom: &[u8],
    remote_hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
    replays: &[crate::replay::Replay],
    selected_rounds: &[Vec<bool>],
    output_path: &std::path::Path,
    settings: &Settings,
    progress_callback: impl Fn(usize, usize),
) -> anyhow::Result<()> {
    let mut vbuf = image::RgbaImage::new(mgba::gba::SCREEN_WIDTH, mgba::gba::SCREEN_HEIGHT);

    let video_output = tempfile::NamedTempFile::new()?;
    let mut video_child = make_video_ffmpeg(
        &settings.ffmpeg,
        video_output.path(),
        mgba::gba::SCREEN_WIDTH as usize,
        mgba::gba::SCREEN_HEIGHT as usize,
        &shell_words::split(&settings.ffmpeg_video_flags)?
            .into_iter()
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>(),
    )?;

    let audio_output = tempfile::NamedTempFile::new()?;
    let mut audio_child = make_audio_ffmpeg(
        &settings.ffmpeg,
        audio_output.path(),
        &shell_words::split(&settings.ffmpeg_audio_flags)?
            .into_iter()
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>(),
    )?;

    let total_frames: usize = replays
        .iter()
        .enumerate()
        .map(|(idx, replay)| kept_input_pairs(replay, selected_rounds.get(idx).map(Vec::as_slice).unwrap_or(&[])))
        .sum();
    let mut completed_total = 0;
    let mut samples = vec![0i16; SAMPLE_RATE as usize];

    let mut resampler = mgba::audio::AudioResampler::new();
    let mut dest_buffer = mgba::audio::AudioBuffer::new(0x4000, AUDIO_CHANNELS as u32);
    let mut prev_should_write = false;

    for (replay_idx, replay) in replays.iter().enumerate() {
        let (mut core, state) = make_core_and_state(
            local_rom,
            &replay.local_sram_dump()?,
            local_hooks,
            remote_rom,
            remote_hooks,
            replay,
            settings,
        )?;
        let full_replay_len = replay.total_input_pairs();
        let selected = selected_rounds.get(replay_idx).map(Vec::as_slice).unwrap_or(&[]);
        let last_selected_round = selected.iter().rposition(|&s| s);
        let kept_replay_len = kept_input_pairs(replay, selected);

        let last_round_idx = replay.rounds.len().saturating_sub(1);
        // For incomplete replays, the stepper can sit in a half-
        // started state forever (e.g. BN3's last partial round
        // waits on a round_start hook that never fires because the
        // recording was cut short). `pairs_left == 0` only fires
        // once the stepper has actually consumed every queued
        // input; if it never enters the consuming state, that
        // check never trips. Watch for "no pair consumed in the
        // last second" as a separate end-of-replay signal: in a
        // healthy run the stepper consumes ~60 pairs/sec, so a
        // full second of stillness means we're stuck.
        // Complete replays don't take this path because
        // `replay.is_complete` is true.
        const NO_PROGRESS_FRAME_BUDGET: u32 = 60;
        let mut last_pairs_left: Option<usize> = None;
        let mut no_progress_frames: u32 = 0;
        loop {
            let (cur_round_idx, is_ended, pairs_left) = {
                let state = state.lock_inner();
                (state.current_round_index() as usize, state.is_round_ended(), state.total_input_pairs_left())
            };

            if (!replay.is_complete && pairs_left == 0) || (is_ended && cur_round_idx >= last_round_idx) {
                break;
            }

            if !replay.is_complete {
                if last_pairs_left == Some(pairs_left) {
                    no_progress_frames += 1;
                    if no_progress_frames >= NO_PROGRESS_FRAME_BUDGET {
                        log::info!(
                            "incomplete replay export: stepper stuck at round {} with {} pairs unconsumed for {} frames, stopping",
                            cur_round_idx, pairs_left, NO_PROGRESS_FRAME_BUDGET,
                        );
                        break;
                    }
                } else {
                    no_progress_frames = 0;
                }
                last_pairs_left = Some(pairs_left);
            }

            if last_selected_round.is_none_or(|last| cur_round_idx > last) {
                break;
            }

            if let Some(err) = state.lock_inner().take_error() {
                Err(err)?;
            }

            let should_write = selected.get(cur_round_idx).copied().unwrap_or(false);
            if should_write && !prev_should_write {
                resampler = mgba::audio::AudioResampler::new();
                dest_buffer.clear();
            }
            prev_should_write = should_write;

            let samples = run_frame(&mut core, &mut resampler, &mut dest_buffer, &mut samples, &mut vbuf);

            if should_write {
                video_child.stdin.as_mut().unwrap().write_all(&vbuf).await?;

                let mut audio_bytes = vec![0u8; samples.len() * 2];
                byteorder::LittleEndian::write_i16_into(samples, &mut audio_bytes[..]);
                audio_child.stdin.as_mut().unwrap().write_all(&audio_bytes).await?;
            }
            progress_callback(
                full_replay_len - state.lock_inner().total_input_pairs_left() + completed_total,
                total_frames,
            );
        }

        completed_total += kept_replay_len;
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
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>(),
    )?;
    mux_child.wait().await?;

    Ok(())
}

pub async fn export_twosided(
    local_rom: &[u8],
    local_hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
    remote_rom: &[u8],
    remote_hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
    replays: &[crate::replay::Replay],
    selected_rounds: &[Vec<bool>],
    output_path: &std::path::Path,
    settings: &Settings,
    progress_callback: impl Fn(usize, usize),
) -> anyhow::Result<()> {
    let mut vbuf = image::RgbaImage::new(mgba::gba::SCREEN_WIDTH, mgba::gba::SCREEN_HEIGHT);
    let mut composed_vbuf = image::RgbaImage::new(mgba::gba::SCREEN_WIDTH * 2, mgba::gba::SCREEN_HEIGHT);

    let video_output = tempfile::NamedTempFile::new()?;
    let mut video_child = make_video_ffmpeg(
        &settings.ffmpeg,
        video_output.path(),
        (mgba::gba::SCREEN_WIDTH * 2) as usize,
        mgba::gba::SCREEN_HEIGHT as usize,
        &shell_words::split(&settings.ffmpeg_video_flags)?
            .into_iter()
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>(),
    )?;

    let local_audio_output = tempfile::NamedTempFile::new()?;
    let mut local_audio_child = make_audio_ffmpeg(
        &settings.ffmpeg,
        local_audio_output.path(),
        &shell_words::split(&settings.ffmpeg_audio_flags)?
            .into_iter()
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>(),
    )?;

    let remote_audio_output = tempfile::NamedTempFile::new()?;
    let mut remote_audio_child = make_audio_ffmpeg(
        &settings.ffmpeg,
        remote_audio_output.path(),
        &shell_words::split(&settings.ffmpeg_audio_flags)?
            .into_iter()
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>(),
    )?;

    let total_frames: usize = replays
        .iter()
        .enumerate()
        .map(|(idx, replay)| kept_input_pairs(replay, selected_rounds.get(idx).map(Vec::as_slice).unwrap_or(&[])))
        .sum();

    let mut completed_total = 0;
    let mut samples = vec![0i16; SAMPLE_RATE as usize];

    let mut local_resampler = mgba::audio::AudioResampler::new();
    let mut local_dest_buffer = mgba::audio::AudioBuffer::new(0x4000, AUDIO_CHANNELS as u32);
    let mut remote_resampler = mgba::audio::AudioResampler::new();
    let mut remote_dest_buffer = mgba::audio::AudioBuffer::new(0x4000, AUDIO_CHANNELS as u32);
    let mut prev_should_write = false;

    for (replay_idx, replay) in replays.iter().enumerate() {
        let local_replay = replay.clone();
        let remote_replay = local_replay.clone().into_remote();

        // For each side's primary core, the shadow runs the OTHER side's
        // ROM + the recording peer's view of their opponent's SRAM.
        let (mut local_core, local_state) = make_core_and_state(
            local_rom,
            &local_replay.local_sram_dump()?,
            local_hooks,
            remote_rom,
            remote_hooks,
            &local_replay,
            settings,
        )?;
        let (mut remote_core, remote_state) = make_core_and_state(
            remote_rom,
            &remote_replay.local_sram_dump()?,
            remote_hooks,
            local_rom,
            local_hooks,
            &remote_replay,
            settings,
        )?;

        let full_replay_len = replay.total_input_pairs();
        let selected = selected_rounds.get(replay_idx).map(Vec::as_slice).unwrap_or(&[]);
        let last_selected_round = selected.iter().rposition(|&s| s);
        let kept_replay_len = kept_input_pairs(replay, selected);
        let last_round_idx = replay.rounds.len().saturating_sub(1);

        loop {
            let cur_round_idx = local_state.lock_inner().current_round_index() as usize;

            {
                let local_state = local_state.lock_inner();
                if (!local_replay.is_complete && local_state.total_input_pairs_left() == 0)
                    || (local_state.is_round_ended() && cur_round_idx >= last_round_idx)
                {
                    break;
                }
            }

            {
                let remote_state = remote_state.lock_inner();
                if (!remote_replay.is_complete && remote_state.total_input_pairs_left() == 0)
                    || (remote_state.is_round_ended() && cur_round_idx >= last_round_idx)
                {
                    break;
                }
            }

            if last_selected_round.is_none_or(|last| cur_round_idx > last) {
                break;
            }
            let should_write = selected.get(cur_round_idx).copied().unwrap_or(false);
            if should_write && !prev_should_write {
                local_resampler = mgba::audio::AudioResampler::new();
                local_dest_buffer.clear();
                remote_resampler = mgba::audio::AudioResampler::new();
                remote_dest_buffer.clear();
            }
            prev_should_write = should_write;

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
                    let local_samples = run_frame(
                        &mut local_core,
                        &mut local_resampler,
                        &mut local_dest_buffer,
                        &mut samples,
                        &mut vbuf,
                    );
                    if should_write {
                        image::imageops::replace(&mut composed_vbuf, &vbuf, 0, 0);
                        let mut audio_bytes = vec![0u8; local_samples.len() * 2];
                        byteorder::LittleEndian::write_i16_into(local_samples, &mut audio_bytes[..]);
                        local_audio_child
                            .stdin
                            .as_mut()
                            .unwrap()
                            .write_all(&audio_bytes)
                            .await?;
                    }
                }

                {
                    let remote_samples = run_frame(
                        &mut remote_core,
                        &mut remote_resampler,
                        &mut remote_dest_buffer,
                        &mut samples,
                        &mut vbuf,
                    );
                    if should_write {
                        image::imageops::replace(&mut composed_vbuf, &vbuf, mgba::gba::SCREEN_WIDTH as i64, 0);
                        let mut audio_bytes = vec![0u8; remote_samples.len() * 2];
                        byteorder::LittleEndian::write_i16_into(remote_samples, &mut audio_bytes[..]);
                        remote_audio_child
                            .stdin
                            .as_mut()
                            .unwrap()
                            .write_all(&audio_bytes)
                            .await?;
                    }
                }

                if should_write {
                    video_child
                        .stdin
                        .as_mut()
                        .unwrap()
                        .write_all(composed_vbuf.as_bytes())
                        .await?;
                }
            }

            while local_state.lock_inner().current_tick() == current_tick {
                run_frame(
                    &mut local_core,
                    &mut local_resampler,
                    &mut local_dest_buffer,
                    &mut samples,
                    &mut vbuf,
                );
            }

            while remote_state.lock_inner().current_tick() == current_tick {
                run_frame(
                    &mut remote_core,
                    &mut remote_resampler,
                    &mut remote_dest_buffer,
                    &mut samples,
                    &mut vbuf,
                );
            }

            progress_callback(
                full_replay_len - local_state.lock_inner().total_input_pairs_left() + completed_total,
                total_frames,
            );
        }

        completed_total += kept_replay_len;
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
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>(),
    )?;
    mux_child.wait().await?;

    Ok(())
}
