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
                // Convert RGB→YUV with the BT.709 matrix in full range, and tag
                // the stream as full-range sRGB (BT.709 primaries + matrix,
                // IEC 61966-2-1 transfer) via `setparams`. Without tags the
                // decoder guesses a steeper "video" gamma and the export looks
                // more saturated than the on-screen sRGB colors; pinning the
                // conversion matrix to the tagged one avoids a hue shift.
                // `-color_*` output options don't stick through the filtergraph
                // (only the matrix/range do), hence `setparams`.
                //
                // The lossless 1× path below stays untagged: gbrp/RGB H.264
                // streams can't carry primaries/transfer at all, so there's no
                // equivalent fix there.
                format!(
                    "-c:v libx264 -vf scale=iw*{f}:ih*{f}:flags=neighbor:out_range=pc:out_color_matrix=bt709,format=yuv420p,setparams=range=pc:colorspace=bt709:color_primaries=bt709:color_trc=iec61966-2-1 -force_key_frames expr:gte(t,n_forced/2) -crf 18 -bf 2",
                    f = factor
                )
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
    let mut core = mgba::core::Core::new_gba(
        "tango",
        &mgba::core::Options {
            audio_sync: true,
            ..Default::default()
        },
    )?;
    core.enable_video_buffer();

    core.as_mut().load_rom(mgba::vfile::VFile::from_vec(rom.to_vec()))?;
    core.as_mut().load_save(mgba::vfile::VFile::from_vec(sram.to_vec()))?;
    // Pin the cart RTC to the recorded match clock so RTC-reading games
    // (exe45) export the same frames the live match produced.
    core.set_rtc_fixed(replay.rtc_time());
    core.as_mut().reset();

    if replay.rounds.is_empty() {
        return Err(anyhow::anyhow!("replay has no rounds"));
    }

    let (stepper_state, _shadow) =
        crate::stepper::State::new_for_replay(replay, shadow_rom, shadow_hooks, Box::new(|| {}))?;
    stepper_state.lock_inner().set_disable_bgm(settings.disable_bgm);

    hooks.install_on_stepper(&mut core, stepper_state.clone());

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

    tango_dataview::rom::bgr555_to_rgba8(core.video_buffer().unwrap(), emu_vbuf);
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

fn split_flags(flags: &str) -> anyhow::Result<Vec<std::ffi::OsString>> {
    Ok(shell_words::split(flags)?
        .into_iter()
        .map(std::ffi::OsString::from)
        .collect())
}

/// Progress denominator: kept input pairs across every replay.
fn total_kept_frames(replays: &[crate::replay::Replay], selected_rounds: &[Vec<bool>]) -> usize {
    replays
        .iter()
        .enumerate()
        .map(|(idx, replay)| kept_input_pairs(replay, selected_rounds.get(idx).map(Vec::as_slice).unwrap_or(&[])))
        .sum()
}

/// Whether playback of this replay is done. An incomplete replay stops
/// the moment the stepper has fed the last queued pair (no
/// fade-to-black tail to wait for); a complete one waits for the final
/// round to actually end + play through the fade. `is_complete` here
/// means "the match was played out", not "the writer finished cleanly"
/// — see `Match::finish_replay`'s call site in pvp_session.rs.
fn playback_finished(
    replay: &crate::replay::Replay,
    state: &crate::stepper::State,
    cur_round_idx: usize,
    last_round_idx: usize,
) -> bool {
    let state = state.lock_inner();
    (!replay.is_complete && state.total_input_pairs_left() == 0)
        || (state.is_round_ended() && cur_round_idx >= last_round_idx)
}

/// Per-replay bookkeeping derived from the export's round selection.
struct ReplayPlan<'a> {
    /// Which rounds get written. Unselected rounds still simulate (the
    /// sim state has to advance through them) — they just don't reach
    /// the encoders.
    selected: &'a [bool],
    last_selected_round: Option<usize>,
    /// Input pairs counted toward progress: every round up to and
    /// including the last selected one.
    kept_replay_len: usize,
    full_replay_len: usize,
    last_round_idx: usize,
}

impl<'a> ReplayPlan<'a> {
    fn new(replay: &crate::replay::Replay, selected_rounds: &'a [Vec<bool>], replay_idx: usize) -> Self {
        let selected = selected_rounds.get(replay_idx).map(Vec::as_slice).unwrap_or(&[]);
        Self {
            selected,
            last_selected_round: selected.iter().rposition(|&s| s),
            kept_replay_len: kept_input_pairs(replay, selected),
            full_replay_len: replay.total_input_pairs(),
            last_round_idx: replay.rounds.len().saturating_sub(1),
        }
    }

    fn should_write(&self, round_idx: usize) -> bool {
        self.selected.get(round_idx).copied().unwrap_or(false)
    }

    /// True once the playhead has passed the last selected round —
    /// nothing further will be written, so the run can stop.
    fn past_selection(&self, round_idx: usize) -> bool {
        self.last_selected_round.is_none_or(|last| round_idx > last)
    }
}

/// The ffmpeg leg of an export: one video encoder + N audio encoders
/// (one per track), each writing to its own temp file, muxed into the
/// final container once every stream is done.
struct EncodePipeline {
    video: FfmpegChild,
    video_output: tempfile::NamedTempFile,
    audios: Vec<(FfmpegChild, tempfile::NamedTempFile)>,
}

impl EncodePipeline {
    /// Spawn the encoder children. `width_screens` is the output width
    /// in GBA screens (1 = one-sided, 2 = side-by-side).
    fn spawn(
        settings: &Settings,
        canceller: &Canceller,
        width_screens: usize,
        audio_tracks: usize,
    ) -> anyhow::Result<Self> {
        let video_output = tempfile::NamedTempFile::new()?;
        let video = make_video_ffmpeg(
            &settings.ffmpeg,
            video_output.path(),
            mgba::gba::SCREEN_WIDTH as usize * width_screens,
            mgba::gba::SCREEN_HEIGHT as usize,
            &split_flags(&settings.ffmpeg_video_flags)?,
            canceller,
        )?;
        let audios = (0..audio_tracks)
            .map(|_| {
                let output = tempfile::NamedTempFile::new()?;
                let child = make_audio_ffmpeg(
                    &settings.ffmpeg,
                    output.path(),
                    &split_flags(&settings.ffmpeg_audio_flags)?,
                    canceller,
                )?;
                Ok((child, output))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Self {
            video,
            video_output,
            audios,
        })
    }

    fn write_video(&mut self, frame: &[u8]) -> std::io::Result<()> {
        self.video.stdin().write_all(frame)
    }

    /// Write one run of interleaved samples to audio track `track` as
    /// s16le bytes (the format the audio child was spawned to read).
    fn write_audio(&mut self, track: usize, samples: &[i16]) -> std::io::Result<()> {
        let mut bytes = vec![0u8; samples.len() * 2];
        byteorder::LittleEndian::write_i16_into(samples, &mut bytes[..]);
        self.audios[track].0.stdin().write_all(&bytes)
    }

    /// Close every encoder's stdin (EOF makes them finish encoding),
    /// wait for each, then mux the encoded streams into `output_path`
    /// with the audio tracks in spawn order.
    fn finish(self, settings: &Settings, canceller: &Canceller, output_path: &std::path::Path) -> anyhow::Result<()> {
        let Self {
            mut video,
            video_output,
            audios,
        } = self;
        video.close_stdin();
        video.wait()?;
        let mut audio_outputs = Vec::with_capacity(audios.len());
        for (mut child, output) in audios {
            child.close_stdin();
            child.wait()?;
            audio_outputs.push(output);
        }
        let mux_child = make_mux_ffmpeg(
            &settings.ffmpeg,
            output_path,
            video_output.path(),
            &audio_outputs.iter().map(|o| o.path()).collect::<Vec<_>>(),
            &split_flags(&settings.ffmpeg_mux_flags)?,
            canceller,
        )?;
        mux_child.wait()?;
        Ok(())
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
    let mut pipeline = EncodePipeline::spawn(settings, canceller, 1, 1)?;

    let total_frames = total_kept_frames(replays, selected_rounds);
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
            &replay.local_sram,
            local_hooks,
            remote_rom,
            remote_hooks,
            replay,
            settings,
        )?;
        let plan = ReplayPlan::new(replay, selected_rounds, replay_idx);

        loop {
            if canceller.is_cancelled() {
                anyhow::bail!("cancelled");
            }
            let cur_round_idx = state.lock_inner().current_round_index() as usize;
            if playback_finished(replay, &state, cur_round_idx, plan.last_round_idx) {
                break;
            }

            if plan.past_selection(cur_round_idx) {
                break;
            }

            if let Some(err) = state.lock_inner().take_error() {
                Err(err)?;
            }

            let should_write = plan.should_write(cur_round_idx);
            if should_write && !prev_should_write {
                resampler = mgba::audio::AudioResampler::new();
                dest_buffer.clear();
            }
            prev_should_write = should_write;

            let samples = run_frame(&mut core, &mut resampler, &mut dest_buffer, &mut samples, &mut vbuf);

            if should_write {
                pipeline.write_video(&vbuf)?;
                pipeline.write_audio(0, samples)?;
            }
            progress_callback(
                plan.full_replay_len - state.lock_inner().total_input_pairs_left() + completed_total,
                total_frames,
            );
        }

        completed_total += plan.kept_replay_len;
    }

    pipeline.finish(settings, canceller, output_path)
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

    // Side-by-side video, two audio tracks: local first, remote second
    // — the mux maps them in spawn order.
    let mut pipeline = EncodePipeline::spawn(settings, canceller, 2, 2)?;

    let total_frames = total_kept_frames(replays, selected_rounds);
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
            &local_replay.local_sram,
            local_hooks,
            remote_rom,
            remote_hooks,
            &local_replay,
            settings,
        )?;
        let (mut remote_core, remote_state) = make_core_and_state(
            remote_rom,
            &remote_replay.local_sram,
            remote_hooks,
            local_rom,
            local_hooks,
            &remote_replay,
            settings,
        )?;

        let plan = ReplayPlan::new(replay, selected_rounds, replay_idx);

        loop {
            if canceller.is_cancelled() {
                anyhow::bail!("cancelled");
            }
            let cur_round_idx = local_state.lock_inner().current_round_index() as usize;

            if playback_finished(&local_replay, &local_state, cur_round_idx, plan.last_round_idx)
                || playback_finished(&remote_replay, &remote_state, cur_round_idx, plan.last_round_idx)
            {
                break;
            }

            if plan.past_selection(cur_round_idx) {
                break;
            }
            let should_write = plan.should_write(cur_round_idx);
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
                        pipeline.write_audio(0, local_samples)?;
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
                        pipeline.write_audio(1, remote_samples)?;
                    }
                }

                if should_write {
                    pipeline.write_video(composed_vbuf.as_bytes())?;
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
                plan.full_replay_len - local_state.lock_inner().total_input_pairs_left() + completed_total,
                total_frames,
            );
        }

        completed_total += plan.kept_replay_len;
    }

    pipeline.finish(settings, canceller, output_path)
}
