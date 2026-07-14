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

const AUDIO_CHANNELS: usize = 2;

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

fn split_flags(flags: &str) -> anyhow::Result<Vec<std::ffi::OsString>> {
    Ok(shell_words::split(flags)?
        .into_iter()
        .map(std::ffi::OsString::from)
        .collect())
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

/// Export an SIO replay ([`crate::replay::VERSION`]): one linear
/// pair re-sim produces both perspectives at once, so the two-sided
/// layout is a compose of the two framebuffers rather than a second
/// simulation. Round boundaries come from RAM-poll telemetry (the
/// stream itself is one continuous run), and `rounds_mask` gates which
/// telemetry rounds reach the encoders — indices match the stats/tab
/// round ordering. Unselected spans still simulate; they just aren't
/// written.
#[allow(clippy::too_many_arguments)]
pub fn export(
    config: &crate::playback::BootConfig,
    inputs: &[[u32; 2]],
    local_player: usize,
    rounds_mask: &[bool],
    output_path: &std::path::Path,
    settings: &Settings,
    canceller: &Canceller,
    progress_callback: impl Fn(usize, usize),
    twosided: bool,
) -> anyhow::Result<()> {
    if canceller.is_cancelled() {
        anyhow::bail!("cancelled");
    }
    anyhow::ensure!(local_player < 2, "bad local player index");
    let last_selected = rounds_mask.iter().rposition(|&s| s);

    let (w, h) = (mgba::gba::SCREEN_WIDTH, mgba::gba::SCREEN_HEIGHT);
    let mut vbuf = image::RgbaImage::new(w, h);
    let mut composed_vbuf = image::RgbaImage::new(w * 2, h);
    let (width_screens, audio_tracks) = if twosided { (2, 2) } else { (1, 1) };
    let mut pipeline = EncodePipeline::spawn(settings, canceller, width_screens, audio_tracks)?;

    // Boot + prime. This is ffmpeg-free but bounded (~a few hundred
    // ticks), so a cancel lands at the next loop check.
    let lifecycle = crate::telemetry::LifecycleSink::new();
    let mut pb = crate::playback::Playback::new(config, Arc::new(inputs.to_vec()), &lifecycle)?;
    // Drop the audio priming piled up (nothing drained during boot).
    for i in 0..2 {
        pb.pair_mut().core_mut(i).audio_buffer().clear();
    }

    let (mut observer, telemetry_store) = crate::telemetry::Telemetry::new(
        [config.support[0].core_poller(0), config.support[1].core_poller(1)],
        lifecycle,
    );

    let mut samples = vec![0i16; SAMPLE_RATE as usize];
    let mut resamplers = [mgba::audio::AudioResampler::new(), mgba::audio::AudioResampler::new()];
    let mut dest_buffers = [
        mgba::audio::AudioBuffer::new(0x4000, AUDIO_CHANNELS as u32),
        mgba::audio::AudioBuffer::new(0x4000, AUDIO_CHANNELS as u32),
    ];
    let mut prev_should_write = false;
    // Telemetry round ordinal: Started events increment it; frames
    // before the first Started belong to round 0.
    let mut rounds_started = 0usize;

    let total = pb.total() as usize;
    while pb.step() {
        if canceller.is_cancelled() {
            anyhow::bail!("cancelled");
        }
        let tick = pb.cursor();
        mgba_siolink::session::TickObserver::on_tick(&mut observer, pb.pair_mut(), tick);
        let (_, events) = telemetry_store.lock().unwrap().drain_confirmed(tick);
        for (_, event) in events {
            if let crate::telemetry::RoundEvent::Started = event {
                rounds_started += 1;
            }
        }
        let cur_round = rounds_started.saturating_sub(1);
        if last_selected.is_none_or(|last| cur_round > last) {
            break;
        }

        let should_write = rounds_mask.get(cur_round).copied().unwrap_or(false);
        if should_write && !prev_should_write {
            for (r, d) in resamplers.iter_mut().zip(dest_buffers.iter_mut()) {
                *r = mgba::audio::AudioResampler::new();
                d.clear();
            }
        }
        prev_should_write = should_write;

        // Drain + resample each core's tick of audio; blit its frame.
        // Track/screen order: local perspective first.
        let order: [usize; 2] = [local_player, 1 - local_player];
        for (slot, &core_idx) in order.iter().enumerate() {
            if !twosided && slot > 0 {
                break;
            }
            let pair = pb.pair_mut();
            let n = {
                let mut core = pair.core_mut(core_idx);
                let core_rate = core.as_ref().audio_sample_rate() as f64;
                let mut core_buffer = core.audio_buffer();
                resamplers[slot].set_source(&mut core_buffer, core_rate, true);
                resamplers[slot].set_destination(&mut dest_buffers[slot], SAMPLE_RATE);
                resamplers[slot].process();
                let cap = samples.len() / AUDIO_CHANNELS;
                let frames = dest_buffers[slot].available().min(cap);
                dest_buffers[slot].read(&mut samples[..frames * AUDIO_CHANNELS], frames);
                frames
            };
            if should_write {
                pipeline.write_audio(slot, &samples[..n * AUDIO_CHANNELS])?;
                if let Some(fb) = pair.video_buffer(core_idx) {
                    tango_dataview::rom::bgr555_to_rgba8(fb, &mut vbuf);
                    if twosided {
                        image::imageops::replace(&mut composed_vbuf, &vbuf, (slot as i64) * w as i64, 0);
                    }
                }
            }
        }
        if should_write {
            if twosided {
                pipeline.write_video(composed_vbuf.as_bytes())?;
            } else {
                pipeline.write_video(&vbuf)?;
            }
        }
        progress_callback(tick as usize, total);
    }

    pipeline.finish(settings, canceller, output_path)
}
