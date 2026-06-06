use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use byteorder::ByteOrder;
use image::EncodableLayout;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// Caller-side cancel handle. `kill()` does two things:
///
///   * Sets an internal flag the export checks every iteration (and
///     at each ffmpeg-free boundary: start of function, start of
///     each per-replay setup) so a cancel takes effect promptly
///     regardless of which phase the export is in — pre-ffmpeg,
///     between replays, or mid-encode-loop during a skipped round
///     where no pipe writes are happening.
///   * Terminates every ffmpeg subprocess that has been registered,
///     so the encode loop's current pipe write (if mid-write) and
///     the post-loop `wait()` on each child return Err immediately.
///
/// Either signal alone is enough to unblock the export; both fire
/// from one `kill()` so neither has to cover the other's gap.
#[derive(Clone, Default)]
pub struct Canceller {
    inner: Arc<CancellerInner>,
}

impl std::fmt::Debug for Canceller {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Canceller").finish_non_exhaustive()
    }
}

#[derive(Default)]
struct CancellerInner {
    cancelled: AtomicBool,
    children: std::sync::Mutex<Vec<Arc<std::sync::Mutex<Option<std::process::Child>>>>>,
}

impl Canceller {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark this canceller cancelled and kill every ffmpeg subprocess
    /// it has registered. Safe to call from any thread, multiple times.
    pub fn kill(&self) {
        self.inner.cancelled.store(true, Ordering::Relaxed);
        for slot in self.inner.children.lock().unwrap().iter() {
            if let Some(child) = slot.lock().unwrap().as_mut() {
                let _ = child.kill();
            }
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Relaxed)
    }

    fn register(&self, child: std::process::Child) -> Arc<std::sync::Mutex<Option<std::process::Child>>> {
        let slot = Arc::new(std::sync::Mutex::new(Some(child)));
        self.inner.children.lock().unwrap().push(slot.clone());
        // Race guard: if kill() already fired before this child was
        // registered, kill it on arrival so it doesn't slip the net.
        if self.inner.cancelled.load(Ordering::Relaxed) {
            if let Some(c) = slot.lock().unwrap().as_mut() {
                let _ = c.kill();
            }
        }
        slot
    }
}

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
            // Scaled exports mux into .mp4 (faststart for streaming);
            // lossless exports mux into .mkv, which takes none of these
            // mp4-only flags. The output extension is chosen by the caller
            // (app.rs save dialog) to match.
            ffmpeg_mux_flags: if factor.is_some() {
                "-movflags +faststart -strict -2".to_string()
            } else {
                String::new()
            },
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
    let shadow = std::sync::Arc::new(std::sync::Mutex::new(shadow));

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
        let mut core_buffer = core.audio_buffer();
        resampler.set_source(&mut core_buffer, core_rate, true);
        resampler.set_destination(dest_buffer, SAMPLE_RATE);
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

/// RAII wrapper around an ffmpeg subprocess. Holds the stdin handle
/// directly (so the encode loop writes without locking) and a shared
/// `Arc<Mutex<Option<Child>>>` slot registered with the `Canceller`
/// — `Canceller::kill()` reaches in through that slot to terminate
/// the process. On Drop (early return / panic / cancel) the wrapper
/// kills + reaps the child if it hasn't been waited for yet.
struct FfmpegChild {
    slot: Arc<std::sync::Mutex<Option<std::process::Child>>>,
    stdin: Option<std::process::ChildStdin>,
}

impl FfmpegChild {
    fn from_spawn(mut child: std::process::Child, canceller: &Canceller) -> Self {
        let stdin = child.stdin.take();
        let slot = canceller.register(child);
        Self { slot, stdin }
    }

    fn stdin(&mut self) -> &mut std::process::ChildStdin {
        self.stdin.as_mut().expect("ffmpeg stdin closed")
    }

    /// Drop the stdin handle so ffmpeg sees EOF and finishes encoding.
    fn close_stdin(&mut self) {
        self.stdin.take();
    }

    /// Block until ffmpeg exits. If the canceller already killed the
    /// process, `wait()` returns with a non-success status and we
    /// surface that as Err — no polling, no fixed cancel latency.
    fn wait(self) -> anyhow::Result<()> {
        let taken = self.slot.lock().unwrap().take();
        let mut child = taken.ok_or_else(|| anyhow::anyhow!("ffmpeg child already taken"))?;
        let status = child.wait()?;
        if !status.success() {
            anyhow::bail!("ffmpeg exited with status {status:?}");
        }
        Ok(())
    }
}

impl Drop for FfmpegChild {
    fn drop(&mut self) {
        let taken = self.slot.lock().unwrap().take();
        if let Some(mut child) = taken {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn make_video_ffmpeg(
    ffmpeg: &Option<std::path::PathBuf>,
    output_path: &std::path::Path,
    width: usize,
    height: usize,
    flags: &[std::ffi::OsString],
    canceller: &Canceller,
) -> anyhow::Result<FfmpegChild> {
    let mut child = std::process::Command::new(resolve_ffmpeg_path(ffmpeg));
    child
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
    Ok(FfmpegChild::from_spawn(child.spawn()?, canceller))
}

fn make_audio_ffmpeg(
    ffmpeg: &Option<std::path::PathBuf>,
    output_path: &std::path::Path,
    flags: &[std::ffi::OsString],
    canceller: &Canceller,
) -> anyhow::Result<FfmpegChild> {
    let mut child = std::process::Command::new(resolve_ffmpeg_path(ffmpeg));
    child
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
    Ok(FfmpegChild::from_spawn(child.spawn()?, canceller))
}

fn make_mux_ffmpeg(
    ffmpeg: &Option<std::path::PathBuf>,
    output_path: &std::path::Path,
    video_input_path: &std::path::Path,
    audio_input_paths: &[&std::path::Path],
    flags: &[std::ffi::OsString],
    canceller: &Canceller,
) -> anyhow::Result<FfmpegChild> {
    let mut child = std::process::Command::new(resolve_ffmpeg_path(ffmpeg));
    child.args(["-y"]).args(["-i"]).arg(video_input_path);

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
    Ok(FfmpegChild::from_spawn(child.spawn()?, canceller))
}

fn kept_input_pairs(replay: &crate::replay::Replay, selected: &[bool]) -> usize {
    match selected.iter().rposition(|&s| s) {
        Some(last) => replay.rounds.iter().take(last + 1).map(|r| r.len()).sum(),
        None => 0,
    }
}

pub fn export(
    local_rom: &[u8],
    local_hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
    remote_rom: &[u8],
    remote_hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
    replays: &[crate::replay::Replay],
    selected_rounds: &[Vec<bool>],
    output_path: &std::path::Path,
    settings: &Settings,
    canceller: &Canceller,
    progress_callback: impl Fn(usize, usize),
) -> anyhow::Result<()> {
    // Boundary check: caller may have flipped the canceller before
    // we got here. Without this, a cancel that arrives in the gap
    // between thread spawn and the first ffmpeg subprocess running
    // has nothing to act on.
    if canceller.is_cancelled() {
        anyhow::bail!("cancelled");
    }
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
        canceller,
    )?;

    let audio_output = tempfile::NamedTempFile::new()?;
    let mut audio_child = make_audio_ffmpeg(
        &settings.ffmpeg,
        audio_output.path(),
        &shell_words::split(&settings.ffmpeg_audio_flags)?
            .into_iter()
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>(),
        canceller,
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
        // Boundary check: per-replay setup (core init + shadow stepper
        // construction) runs ffmpeg-free, so the kill mechanism can't
        // reach it. One check before each setup is enough.
        if canceller.is_cancelled() {
            anyhow::bail!("cancelled");
        }
        let (mut core, state) = make_core_and_state(
            local_rom,
            &replay.local_sram_dump(),
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
        loop {
            if canceller.is_cancelled() {
                anyhow::bail!("cancelled");
            }
            let (cur_round_idx, is_ended, pairs_left) = {
                let state = state.lock_inner();
                (
                    state.current_round_index() as usize,
                    state.is_round_ended(),
                    state.total_input_pairs_left(),
                )
            };

            // Incomplete: stop the moment the stepper has fed the
            // last queued pair (no fade-to-black tail to wait for).
            // Complete: wait for the final round to actually end +
            // play through the fade. `is_complete` here means
            // "the match was played out", not "the writer finished
            // cleanly" — see `Match::finish_replay`'s call site
            // in pvp_session.rs.
            if (!replay.is_complete && pairs_left == 0) || (is_ended && cur_round_idx >= last_round_idx) {
                break;
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
                video_child.stdin().write_all(&vbuf)?;

                let mut audio_bytes = vec![0u8; samples.len() * 2];
                byteorder::LittleEndian::write_i16_into(samples, &mut audio_bytes[..]);
                audio_child.stdin().write_all(&audio_bytes)?;
            }
            progress_callback(
                full_replay_len - state.lock_inner().total_input_pairs_left() + completed_total,
                total_frames,
            );
        }

        completed_total += kept_replay_len;
    }

    video_child.close_stdin();
    video_child.wait()?;
    audio_child.close_stdin();
    audio_child.wait()?;

    let mux_child = make_mux_ffmpeg(
        &settings.ffmpeg,
        output_path,
        video_output.path(),
        &[audio_output.path()],
        &shell_words::split(&settings.ffmpeg_mux_flags)?
            .into_iter()
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>(),
        canceller,
    )?;
    mux_child.wait()?;

    Ok(())
}

pub fn export_twosided(
    local_rom: &[u8],
    local_hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
    remote_rom: &[u8],
    remote_hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
    replays: &[crate::replay::Replay],
    selected_rounds: &[Vec<bool>],
    output_path: &std::path::Path,
    settings: &Settings,
    canceller: &Canceller,
    progress_callback: impl Fn(usize, usize),
) -> anyhow::Result<()> {
    // See `export` — boundary check before the first ffmpeg spawns.
    if canceller.is_cancelled() {
        anyhow::bail!("cancelled");
    }
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
        canceller,
    )?;

    let local_audio_output = tempfile::NamedTempFile::new()?;
    let mut local_audio_child = make_audio_ffmpeg(
        &settings.ffmpeg,
        local_audio_output.path(),
        &shell_words::split(&settings.ffmpeg_audio_flags)?
            .into_iter()
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>(),
        canceller,
    )?;

    let remote_audio_output = tempfile::NamedTempFile::new()?;
    let mut remote_audio_child = make_audio_ffmpeg(
        &settings.ffmpeg,
        remote_audio_output.path(),
        &shell_words::split(&settings.ffmpeg_audio_flags)?
            .into_iter()
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>(),
        canceller,
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
        // See `export` — boundary check before each ffmpeg-free
        // per-replay setup.
        if canceller.is_cancelled() {
            anyhow::bail!("cancelled");
        }
        let local_replay = replay.clone();
        let remote_replay = local_replay.clone().into_remote();

        // For each side's primary core, the shadow runs the OTHER side's
        // ROM + the recording peer's view of their opponent's SRAM.
        let (mut local_core, local_state) = make_core_and_state(
            local_rom,
            &local_replay.local_sram_dump(),
            local_hooks,
            remote_rom,
            remote_hooks,
            &local_replay,
            settings,
        )?;
        let (mut remote_core, remote_state) = make_core_and_state(
            remote_rom,
            &remote_replay.local_sram_dump(),
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
            if canceller.is_cancelled() {
                anyhow::bail!("cancelled");
            }
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
                        local_audio_child.stdin().write_all(&audio_bytes)?;
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
                        remote_audio_child.stdin().write_all(&audio_bytes)?;
                    }
                }

                if should_write {
                    video_child.stdin().write_all(composed_vbuf.as_bytes())?;
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

    video_child.close_stdin();
    video_child.wait()?;
    local_audio_child.close_stdin();
    local_audio_child.wait()?;
    remote_audio_child.close_stdin();
    remote_audio_child.wait()?;

    let mux_child = make_mux_ffmpeg(
        &settings.ffmpeg,
        output_path,
        video_output.path(),
        &[local_audio_output.path(), remote_audio_output.path()],
        &shell_words::split(&settings.ffmpeg_mux_flags)?
            .into_iter()
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>(),
        canceller,
    )?;
    mux_child.wait()?;

    Ok(())
}
