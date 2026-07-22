//! Native replay → video export: the desktop client's ffmpeg
//! subprocess pipeline. Video and audio each stream through their own
//! encoder process into temp files (raw RGBA / s16le on stdin — you
//! can only pipe one stream per process portably), then a third
//! process muxes with `-c copy`. The emulation + pipe-writing loop
//! runs on a dedicated thread (pipe writes block when the encoder
//! falls behind; the UI thread must not), reporting progress back to
//! the shared signals through a channel.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use dioxus::prelude::*;

use super::{expand_and_scale, Progress, EXPORT_CANCEL, EXPORT_PROGRESS, OPUS_RATE, OUT_H, OUT_W};

/// Where the export streams to.
pub enum ExportTarget {
    /// The user's own file, picked up front.
    Picked(PathBuf),
    /// Never constructed natively (the picker always exists); present
    /// so the shared UI's fallback arm compiles.
    #[allow(dead_code)]
    OpfsTemp(crate::storage::Storage),
}

/// Whether an up-front save picker exists. Always on native (rfd).
pub fn save_picker_available() -> bool {
    true
}

/// Pick the output file via the native save dialog. The suggested
/// `.webm` name becomes `.mp4` — the native pipeline encodes
/// x264/aac like the desktop's scaled export, which WebM can't carry.
pub async fn pick_save_file(suggested: &str) -> anyhow::Result<Option<PathBuf>> {
    let suggested = suggested.strip_suffix(".webm").map(|s| format!("{s}.mp4")).unwrap_or_else(|| suggested.to_owned());
    Ok(rfd::AsyncFileDialog::new()
        .set_file_name(suggested)
        .add_filter("MP4 video", &["mp4"])
        .save_file()
        .await
        .map(|h| h.path().to_path_buf()))
}

/// Render `replay` into `target`. The blocking pipeline runs on its
/// own thread; this future relays progress + cancellation and resolves
/// when the mux completes.
pub async fn export_replay(
    replay: tango_pvp::replay::Replay,
    local_rom: Vec<u8>,
    remote_rom: Vec<u8>,
    _file_stem: String,
    target: ExportTarget,
    range: Option<(u32, u32)>,
) -> anyhow::Result<()> {
    let output = match target {
        ExportTarget::Picked(path) => path,
        ExportTarget::OpfsTemp(_) => anyhow::bail!("no save picker available"),
    };

    let stream_len = replay.inputs.len();
    let (start, end) = match range {
        Some((s, e)) => ((s as usize).min(stream_len), (e as usize).clamp(s as usize, stream_len)),
        None => (0, stream_len),
    };
    let total = end - start;
    if total == 0 {
        anyhow::bail!("empty export range");
    }
    *EXPORT_CANCEL.write() = false;
    *EXPORT_PROGRESS.write() = Some(Progress { frame: 0, total });

    let cancel = Arc::new(AtomicBool::new(false));
    let (prog_tx, prog_rx) = std::sync::mpsc::channel::<usize>();
    let (done_tx, done_rx) = futures::channel::oneshot::channel::<anyhow::Result<()>>();
    {
        let cancel = cancel.clone();
        std::thread::spawn(move || {
            let _ = done_tx.send(run_blocking(replay, local_rom, remote_rom, &output, start, end, &cancel, prog_tx));
        });
    }

    // Relay progress + the cancel flag until the worker finishes.
    let mut done_rx = done_rx;
    let result = loop {
        match futures::poll!(&mut done_rx) {
            std::task::Poll::Ready(r) => break r.unwrap_or_else(|_| Err(anyhow::anyhow!("export thread died"))),
            std::task::Poll::Pending => {}
        }
        if *EXPORT_CANCEL.peek() {
            cancel.store(true, Ordering::Relaxed);
        }
        if let Some(frame) = prog_rx.try_iter().last() {
            *EXPORT_PROGRESS.write() = Some(Progress { frame, total });
        }
        crate::compat::sleep_ms(100).await;
    };
    *EXPORT_PROGRESS.write() = None;
    result
}

/// An ffmpeg child whose stdin the encode loop writes; killed + reaped
/// on drop (early return / cancel), waited explicitly on success.
struct Ffmpeg {
    child: Option<Child>,
    stdin: Option<std::process::ChildStdin>,
}

impl Ffmpeg {
    fn spawn(mut cmd: Command) -> anyhow::Result<Self> {
        let mut child = cmd
            .spawn()
            .map_err(|e| anyhow::anyhow!("couldn't run ffmpeg (is it installed and on PATH?): {e}"))?;
        let stdin = child.stdin.take();
        Ok(Self {
            child: Some(child),
            stdin,
        })
    }

    fn stdin(&mut self) -> &mut std::process::ChildStdin {
        self.stdin.as_mut().expect("ffmpeg stdin closed")
    }

    /// Drop stdin so ffmpeg sees EOF and finishes the encode.
    fn close_stdin(&mut self) {
        self.stdin.take();
    }

    fn wait(mut self) -> anyhow::Result<()> {
        let mut child = self.child.take().expect("ffmpeg already waited");
        let status = child.wait()?;
        if !status.success() {
            anyhow::bail!("ffmpeg exited with status {status:?}");
        }
        Ok(())
    }
}

impl Drop for Ffmpeg {
    fn drop(&mut self) {
        self.stdin.take();
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// `ffmpeg` beside our exe if present, else from PATH — the desktop's
/// sidecar discovery.
fn ffmpeg_path() -> PathBuf {
    let mut p = std::env::current_exe()
        .ok()
        .as_ref()
        .and_then(|p| p.parent())
        .map(|p| p.join("ffmpeg"))
        .unwrap_or_else(|| "ffmpeg".into());
    p.set_extension(std::env::consts::EXE_EXTENSION);
    if p.exists() {
        p
    } else {
        "ffmpeg".into()
    }
}

fn base_command() -> Command {
    let mut cmd = Command::new(ffmpeg_path());
    cmd.stdin(Stdio::piped()).arg("-y");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    cmd
}

#[allow(clippy::too_many_arguments)]
fn run_blocking(
    replay: tango_pvp::replay::Replay,
    local_rom: Vec<u8>,
    remote_rom: Vec<u8>,
    output: &Path,
    start: usize,
    end: usize,
    cancel: &AtomicBool,
    prog_tx: std::sync::mpsc::Sender<usize>,
) -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let video_tmp = tmp.path().join("video.mkv");
    let audio_tmp = tmp.path().join("audio.mkv");

    // The two encoder processes, fed on stdin. Flags follow the
    // desktop's scaled-export defaults (we pre-scale nearest-neighbor
    // ourselves, so no scale filter).
    let mut video = {
        let mut cmd = base_command();
        cmd.args([
            "-f",
            "rawvideo",
            "-pixel_format",
            "rgba",
            "-video_size",
            &format!("{OUT_W}x{OUT_H}"),
            "-framerate",
            "16777216/280896",
            "-i",
            "pipe:",
        ])
        .args([
            "-c:v",
            "libx264",
            "-vf",
            "format=yuv420p",
            "-force_key_frames",
            "expr:gte(t,n_forced/2)",
            "-crf",
            "18",
            "-bf",
            "2",
        ])
        .args(["-f", "matroska"])
        .arg(&video_tmp);
        Ffmpeg::spawn(cmd)?
    };
    let mut audio = {
        let mut cmd = base_command();
        cmd.args(["-f", "s16le", "-ar", "48k", "-ac", "2", "-i", "pipe:"])
            .args(["-c:a", "aac", "-ar", "48000", "-b:a", "384k", "-ac", "2"])
            .args(["-f", "matroska"])
            .arg(&audio_tmp);
        Ffmpeg::spawn(cmd)?
    };

    // The same boot the viewer uses, minus an audio sink or canvas.
    let (mut playback, local_player, _inputs) = crate::session::replay::boot(&replay, local_rom, remote_rom, false)?;

    // Clip export: fast-skip to the range start under frameskip, then
    // drop the skip-produced audio so the clip opens clean.
    if start > 0 {
        playback.pair_mut().set_frameskip(0, i32::MAX);
        playback.pair_mut().set_frameskip(1, i32::MAX);
        while (playback.cursor() as usize) < start {
            if cancel.load(Ordering::Relaxed) {
                anyhow::bail!("cancelled");
            }
            if !playback.step() {
                break;
            }
            if playback.cursor() % 600 == 0 {
                for i in 0..2 {
                    playback.pair_mut().core_mut(i).audio_buffer().clear();
                }
            }
        }
        playback.pair_mut().set_frameskip(0, 0);
        playback.pair_mut().set_frameskip(1, 0);
        for i in 0..2 {
            playback.pair_mut().core_mut(i).audio_buffer().clear();
        }
    }

    // Audio replumbing: native-rate core output → 48 kHz s16 via the
    // mgba resampler (export wants every sample, 1:1).
    let mut resampler = mgba::audio::AudioResampler::new();
    let dest_capacity = OPUS_RATE as usize;
    let mut dest_buffer = mgba::audio::OwnedAudioBuffer::new(dest_capacity, 2);
    let mut pending_audio: Vec<i16> = Vec::new();

    let mut rgba = vec![0u8; OUT_W * OUT_H * 4];
    let mut frame_idx: usize = 0;

    loop {
        if cancel.load(Ordering::Relaxed) {
            anyhow::bail!("cancelled");
        }
        if playback.cursor() as usize >= end || !playback.step() {
            break;
        }

        let link = playback.pair_mut();

        if let Some(vbuf) = link.video_buffer(local_player) {
            expand_and_scale(vbuf, &mut rgba);
            video.stdin().write_all(&rgba)?;
            frame_idx += 1;
        }

        {
            let rate = link.core(local_player).audio_sample_rate() as f64;
            let core = link.core_mut(local_player);
            let mut source = core.audio_buffer();
            resampler.set_source(&mut source, rate, true);
            resampler.set_destination(&mut dest_buffer, OPUS_RATE);
            resampler.process();
            let available = dest_buffer.available().min(dest_capacity);
            if available > 0 {
                let at = pending_audio.len();
                pending_audio.resize(at + available * 2, 0);
                dest_buffer.read(&mut pending_audio[at..], available);
            }
        }
        // ~100ms batches keep the pipe writes chunky.
        if pending_audio.len() >= (OPUS_RATE as usize / 10) * 2 {
            audio.stdin().write_all(bytemuck::cast_slice(&pending_audio))?;
            pending_audio.clear();
        }

        if frame_idx % 30 == 0 {
            let _ = prog_tx.send(frame_idx);
        }
    }

    if !pending_audio.is_empty() {
        audio.stdin().write_all(bytemuck::cast_slice(&pending_audio))?;
        pending_audio.clear();
    }
    video.close_stdin();
    audio.close_stdin();
    video.wait()?;
    audio.wait()?;

    if cancel.load(Ordering::Relaxed) {
        anyhow::bail!("cancelled");
    }

    // Mux with stream copies; +faststart for streaming-friendly mp4.
    let mux = {
        let mut cmd = base_command();
        cmd.arg("-i")
            .arg(&video_tmp)
            .arg("-i")
            .arg(&audio_tmp)
            .args(["-c:v", "copy", "-c:a", "copy", "-map", "0", "-map", "1", "-movflags", "+faststart"])
            .arg(output);
        Ffmpeg::spawn(cmd)?
    };
    mux.wait()?;
    Ok(())
}
