//! The native encoder backend: one ffmpeg subprocess per stream.
//!
//! Each child reads raw frames or samples on its stdin and writes a
//! fragmented MP4 carrying that one stream on its stdout, which [`fmp4`]
//! reads back as samples. A reader thread drains that stdout
//! continuously (a process can deadlock against its own pipe if the
//! writer waits for the reader while the reader waits for the writer),
//! and the export thread turns those bytes into packets.
//!
//! ffmpeg muxes each stream into a *transport*, never into the output:
//! the container the export actually writes is assembled in
//! [`crate::mux`] from the packets of every stream at once. Asking for a
//! container here rather than a bare elementary stream is what makes
//! that cheap — the fragments state each sample's size, duration and
//! sync flag, and the `moov` states the codec configuration, so nothing
//! has to be recovered by parsing a bitstream.
//!
//! Three flags shape the command lines:
//!
//!   * **No B-frames** (`-bf 0`). Presentation order is storage order
//!     everywhere downstream — [`Packet`] has no separate DTS — so frame
//!     reordering is turned off rather than carried. The cost is a few
//!     percent of bitrate.
//!   * **Encoder-side scaling.** Pre-scaled frames would mean pushing
//!     the upscaled bytes through a pipe — at 10× that is hundreds of
//!     megabytes a second — so the scale filter runs in ffmpeg and the
//!     caller keeps pushing frames at their native size.
//!   * **Timed fragments** ([`FRAGMENT_MICROS`]). Left to itself the
//!     muxer would hold a stream with no keyframes — any audio track —
//!     until the export ended, and the session would hold every video
//!     packet waiting for it.

use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use crate::cancel::ChildSlot;
use crate::settings::AAC_SAMPLES_PER_FRAME;
use crate::{
    AudioCodec, AudioSettings, Backend, Canceller, Error, H264Quality, Packet, Settings, VideoCodec, VideoSettings,
};

pub(crate) mod fmp4;

/// How much media a child may hold before it must write a fragment.
/// Bounds both the reader's memory and how far one stream can lag the
/// others in the session's interleaving.
const FRAGMENT_MICROS: u32 = 250_000;

/// The one output format an export asks ffmpeg for.
const OUTPUT_FORMAT: &str = "mp4";

/// Keep the last of a child's stderr for error messages. A failing
/// ffmpeg explains itself there and nowhere else, and in a GUI build
/// nobody sees the console.
const STDERR_TAIL_LINES: usize = 12;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// One encoded access unit as the encoder's stream described it.
pub struct Sample {
    pub data: Vec<u8>,
    pub keyframe: bool,
    /// Length in the track's timescale.
    pub duration: u64,
}

/// `ffmpeg` beside the running executable if it's there, else whatever
/// `PATH` finds.
pub fn resolve_path(configured: &Option<PathBuf>) -> PathBuf {
    if let Some(path) = configured {
        return path.clone();
    }
    let mut beside = std::env::current_exe()
        .ok()
        .as_ref()
        .and_then(|p| p.parent())
        .map(|p| p.join("ffmpeg"))
        .unwrap_or_else(|| "ffmpeg".into());
    beside.set_extension(std::env::consts::EXE_EXTENSION);
    if beside.exists() {
        beside
    } else {
        "ffmpeg".into()
    }
}

/// Check that this ffmpeg can write the output format an export needs.
///
/// Worth doing up front because a build trimmed down — as a bundled
/// sidecar tends to be — can have every encoder we ask for and still
/// lack the muxer, and ffmpeg's own complaint doesn't say that the
/// *build* is the problem.
pub fn check_formats(ffmpeg: &Path, formats: &[&str]) -> crate::Result<()> {
    let mut command = Command::new(ffmpeg);
    command.args(["-hide_banner", "-muxers"]).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    let output = command.output().map_err(|e| Error::FfmpegSpawn {
        path: ffmpeg.display().to_string(),
        source: e,
    })?;
    let listing = String::from_utf8_lossy(&output.stdout);
    // Lines look like "  E  mp4    MP4 (MPEG-4 Part 14)"; take the name
    // column of anything marked as a muxer.
    let available: Vec<&str> = listing
        .lines()
        .filter_map(|line| {
            let (flags, rest) = line.split_at(line.len().min(4));
            flags.contains('E').then(|| rest.split_whitespace().next()).flatten()
        })
        .collect();
    let missing: Vec<&str> = formats
        .iter()
        .copied()
        .filter(|f| !available.contains(f))
        .collect();
    if !missing.is_empty() {
        return Err(Error::FfmpegMissingFormats {
            formats: missing.join(","),
        });
    }
    Ok(())
}

/// Every output format an export will ask this ffmpeg for.
pub fn required_formats() -> Vec<&'static str> {
    vec![OUTPUT_FORMAT]
}

/// One encoder subprocess and the reader for what it produces.
pub struct Encoder {
    slot: ChildSlot,
    stdin: Option<std::process::ChildStdin>,
    stdout: Receiver<Vec<u8>>,
    stderr: Arc<Mutex<Vec<String>>>,
    reader: Option<std::thread::JoinHandle<()>>,
    stream: fmp4::Reader,
    /// The timebase this track's packets must be in. The child is asked
    /// to write in it, and refuses to be believed until it has.
    timescale: u32,
    next_pts: u64,
}

impl Encoder {
    /// Spawn the video encoder. `width`/`height` are the *input* frame
    /// size; the scale factor is applied inside ffmpeg.
    pub fn video(
        ffmpeg: &Path,
        settings: &VideoSettings,
        width: u32,
        height: u32,
        canceller: &Canceller,
    ) -> crate::Result<Self> {
        let mut command = base_command(ffmpeg);
        command.args([
            "-f",
            "rawvideo",
            "-pixel_format",
            "rgba",
            "-video_size",
            &format!("{width}x{height}"),
            "-framerate",
            &format!("{}/{}", settings.timescale, settings.frame_duration),
            "-i",
            "pipe:",
        ]);
        for arg in video_encode_args(settings) {
            command.arg(arg);
        }
        // Ask for the export's own timebase, so a frame's duration comes
        // back as the exact tick count it was pushed with rather than a
        // rate rounded into whatever unit the muxer would have picked.
        command.args(["-video_track_timescale", &settings.timescale.to_string()]);
        Self::spawn(command, canceller, settings.timescale)
    }

    /// Spawn one audio encoder.
    pub fn audio(ffmpeg: &Path, settings: &AudioSettings, canceller: &Canceller) -> crate::Result<Self> {
        let mut command = base_command(ffmpeg);
        command.args([
            "-f",
            "s16le",
            "-ar",
            &settings.sample_rate.to_string(),
            "-ac",
            &settings.channels.to_string(),
            "-i",
            "pipe:",
        ]);
        for arg in audio_encode_args(settings) {
            command.arg(arg);
        }
        // An audio track's timebase is its sample rate, which is what
        // the MP4 muxer uses unasked — so a packet's duration arrives
        // already counted in samples.
        Self::spawn(command, canceller, settings.sample_rate)
    }

    fn spawn(mut command: Command, canceller: &Canceller, timescale: u32) -> crate::Result<Self> {
        command.args([
            "-movflags",
            "empty_moov+default_base_moof",
            "-frag_duration",
            &FRAGMENT_MICROS.to_string(),
            "-f",
            OUTPUT_FORMAT,
            "pipe:1",
        ]);
        let mut child = command.spawn().map_err(|e| Error::FfmpegSpawn {
            path: command.get_program().to_string_lossy().into_owned(),
            source: e,
        })?;
        let stdin = child.stdin.take();
        let mut child_stdout = child.stdout.take().expect("stdout was piped");
        let child_stderr = child.stderr.take().expect("stderr was piped");

        let (tx, stdout) = std::sync::mpsc::channel();
        let reader = std::thread::Builder::new()
            .name("ffmpeg-stdout".into())
            .spawn(move || {
                // Drain unconditionally: if this thread stops reading
                // while the export keeps writing frames, both sides
                // block on a full pipe and the export hangs.
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    match child_stdout.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if tx.send(buf[..n].to_vec()).is_err() {
                                break;
                            }
                        }
                    }
                }
            })
            .expect("spawn ffmpeg stdout reader");

        let stderr = Arc::new(Mutex::new(Vec::new()));
        {
            let stderr = stderr.clone();
            std::thread::Builder::new()
                .name("ffmpeg-stderr".into())
                .spawn(move || {
                    for line in std::io::BufReader::new(child_stderr).lines().map_while(Result::ok) {
                        let mut tail = stderr.lock().unwrap();
                        if tail.len() == STDERR_TAIL_LINES {
                            tail.remove(0);
                        }
                        tail.push(line);
                    }
                })
                .expect("spawn ffmpeg stderr reader");
        }

        Ok(Self {
            slot: canceller.register(child),
            stdin,
            stdout,
            stderr,
            reader: Some(reader),
            stream: fmp4::Reader::new(),
            timescale,
            next_pts: 0,
        })
    }

    /// Hand raw input to the encoder. Blocks while the encoder is
    /// behind, which is the backpressure that keeps an export from
    /// running ahead of its encoders and into memory.
    pub fn write(&mut self, bytes: &[u8]) -> crate::Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| Error::internal("the encoder's input is already closed"))?;
        match stdin.write_all(bytes) {
            Ok(()) => Ok(()),
            // A dead encoder shows up here as a broken pipe; its own
            // complaint is more useful than ours.
            Err(e) => Err(self.error(format!("couldn't feed ffmpeg: {e}"))),
        }
    }

    /// Packets the encoder has produced so far. Never blocks.
    pub fn poll(&mut self) -> crate::Result<Vec<Packet>> {
        self.read_available()?;
        self.drain_stream()
    }

    /// Codec configuration, once the stream has revealed it.
    pub fn codec_private(&self) -> Option<Vec<u8>> {
        self.stream.codec_private()
    }

    /// Stop feeding the encoder. EOF on stdin is what tells ffmpeg to
    /// flush what it's holding and exit.
    pub fn close_input(&mut self) {
        self.stdin.take();
    }

    /// Wait for the encoder to exit and collect the tail of its stream.
    pub fn finish(&mut self) -> crate::Result<Vec<Packet>> {
        self.close_input();
        if let Some(reader) = self.reader.take() {
            // The reader thread ends at stdout EOF, which happens when
            // the child exits, so this also waits out the encode tail.
            let _ = reader.join();
        }
        self.read_available()?;
        let packets = self.drain_stream()?;

        let taken = self.slot.lock().unwrap().take();
        let mut child = taken.ok_or_else(|| Error::internal("the encoder was already reaped"))?;
        let status = child.wait()?;
        if !status.success() {
            return Err(self.error(format!("ffmpeg exited with {status}")));
        }
        Ok(packets)
    }

    fn read_available(&mut self) -> crate::Result<()> {
        while let Ok(chunk) = self.stdout.try_recv() {
            self.stream.push(&chunk).map_err(|e| self.error(e.to_string()))?;
        }
        Ok(())
    }

    /// Turn the samples read so far into packets, timed by running
    /// total. The durations are the stream's own, in the timebase this
    /// encoder was asked for, so the total stays exact however long an
    /// export runs.
    fn drain_stream(&mut self) -> crate::Result<Vec<Packet>> {
        let samples = self.stream.take_samples();
        if samples.is_empty() {
            return Ok(Vec::new());
        }
        // Timing depends on the child having written the timebase it was
        // asked for; a stream in some other unit would be silently out
        // of sync, so it's refused instead.
        match self.stream.timescale() {
            Some(timescale) if timescale == self.timescale => {}
            other => {
                return Err(self.error(format!(
                    "ffmpeg wrote a stream timed in {other:?} ticks per second, not {}",
                    self.timescale
                )))
            }
        }
        Ok(samples
            .into_iter()
            .map(|sample| {
                let pts = self.next_pts;
                self.next_pts += sample.duration;
                Packet {
                    pts,
                    duration: sample.duration,
                    keyframe: sample.keyframe,
                    data: sample.data,
                }
            })
            .collect())
    }

    /// Attach the encoder's own last words to a failure.
    fn error(&self, context: String) -> Error {
        Error::Ffmpeg {
            context,
            stderr: self.stderr.lock().unwrap().clone(),
        }
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        // An export that ends early (error, cancel, panic) must not
        // leave an encoder running.
        self.stdin.take();
        let taken: Option<Child> = self.slot.lock().unwrap().take();
        if let Some(mut child) = taken {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// The native [`Backend`]: an encoder subprocess per stream.
pub struct FfmpegBackend {
    video: Encoder,
    /// One per audio track.
    audio: Vec<Encoder>,
    audio_codec: AudioCodec,
    /// The tails collected while flushing, which arrive outside a
    /// `poll`.
    ready: Vec<(usize, Packet)>,
    flush_done: Vec<bool>,
}

impl FfmpegBackend {
    /// Spawn the encoders for `settings`. `ffmpeg` selects the binary;
    /// `None` looks beside the running executable and then on `PATH`.
    pub fn new(settings: &Settings, ffmpeg: Option<PathBuf>, canceller: &Canceller) -> crate::Result<Self> {
        settings.validate()?;
        canceller.check()?;
        let path = resolve_path(&ffmpeg);
        check_formats(&path, &required_formats())?;

        let video = Encoder::video(
            &path,
            &settings.video,
            settings.video.width,
            settings.video.height,
            canceller,
        )?;
        let mut audio = Vec::with_capacity(settings.audio_tracks);
        for _ in 0..settings.audio_tracks {
            audio.push(Encoder::audio(&path, &settings.audio, canceller)?);
        }
        Ok(Self {
            video,
            audio,
            audio_codec: settings.audio.codec,
            ready: Vec::new(),
            // Video plus one per audio track.
            flush_done: vec![false; settings.audio_tracks + 1],
        })
    }

    fn encoder(&mut self, track: usize) -> crate::Result<&mut Encoder> {
        if track == crate::VIDEO_TRACK {
            return Ok(&mut self.video);
        }
        self.audio
            .get_mut(track - 1)
            .ok_or_else(|| Error::internal(format!("no track {track}")))
    }
}

impl Backend for FfmpegBackend {
    fn submit_video(&mut self, frame: &[u8]) -> crate::Result<()> {
        self.video.write(frame)
    }

    fn submit_audio(&mut self, track: usize, samples: &[i16]) -> crate::Result<()> {
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        self.encoder(track + 1)?.write(&bytes)
    }

    fn poll(&mut self) -> crate::Result<Vec<(usize, Packet)>> {
        let mut out = std::mem::take(&mut self.ready);
        for packet in self.video.poll()? {
            out.push((crate::VIDEO_TRACK, packet));
        }
        for track in 0..self.audio.len() {
            for packet in self.audio[track].poll()? {
                out.push((track + 1, packet));
            }
        }
        Ok(out)
    }

    fn codec_private(&self, track: usize) -> Option<Vec<u8>> {
        if track == crate::VIDEO_TRACK {
            return self.video.codec_private();
        }
        self.audio.get(track - 1)?.codec_private()
    }

    fn codec_delay_samples(&self, track: usize) -> u64 {
        if track == crate::VIDEO_TRACK {
            return 0;
        }
        match self.audio_codec {
            // ffmpeg's native AAC encoder primes with one full frame,
            // which a player has to discard to stay in sync with the
            // video. Nothing in the stream states it, so it's stated
            // here — the one place that knows which encoder produced it.
            AudioCodec::Aac { .. } => AAC_SAMPLES_PER_FRAME,
            AudioCodec::Flac => 0,
        }
    }

    fn begin_flush(&mut self) -> crate::Result<()> {
        self.video.close_input();
        for encoder in &mut self.audio {
            encoder.close_input();
        }
        Ok(())
    }

    /// Blocks: each encoder is waited out in turn. Safe here because a
    /// native export runs on its own thread, and it means the whole
    /// shutdown completes in one call.
    fn poll_flush(&mut self) -> crate::Result<bool> {
        if !self.flush_done[crate::VIDEO_TRACK] {
            let tail = self.video.finish()?;
            self.ready
                .extend(tail.into_iter().map(|p| (crate::VIDEO_TRACK, p)));
            self.flush_done[crate::VIDEO_TRACK] = true;
        }
        for track in 0..self.audio.len() {
            if self.flush_done[track + 1] {
                continue;
            }
            let tail = self.audio[track].finish()?;
            self.ready.extend(tail.into_iter().map(|p| (track + 1, p)));
            self.flush_done[track + 1] = true;
        }
        Ok(true)
    }
}

fn base_command(ffmpeg: &Path) -> Command {
    let mut command = Command::new(ffmpeg);
    command
        .args(["-y", "-hide_banner", "-loglevel", "error", "-nostdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

fn video_encode_args(settings: &VideoSettings) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    let scale = settings.scale;
    let VideoCodec::H264 { quality } = settings.codec;
    match quality {
        H264Quality::Lossless => {
            // RGB in, RGB out: no color conversion, nothing to lose. An
            // RGB H.264 stream can't carry color tags at all, which is
            // why this path sets none.
            args.extend(["-c:v", "libx264rgb", "-preset", "ultrafast", "-qp", "0"].map(String::from));
            if scale > 1 {
                args.extend(["-vf".into(), format!("scale=iw*{scale}:ih*{scale}:flags=neighbor")]);
            }
        }
        H264Quality::Crf(crf) => {
            args.extend(["-c:v", "libx264"].map(String::from));
            args.extend(["-vf".into(), yuv_filter_chain(scale, settings)]);
            args.extend(["-crf".into(), crf.to_string()]);
        }
        H264Quality::Bitrate(bits) => {
            args.extend(["-c:v", "libx264"].map(String::from));
            args.extend(["-vf".into(), yuv_filter_chain(scale, settings)]);
            args.extend(["-b:v".into(), bits.to_string()]);
        }
    }
    // Frame reordering has nowhere to go downstream; see the module docs.
    args.extend(["-bf".into(), "0".into()]);
    args.extend(["-g".into(), settings.keyframe_interval.max(1).to_string()]);
    args
}

/// Scale (nearest-neighbor, to keep pixel art crisp) and convert to
/// 4:2:0, tagging the result as what it is.
///
/// The tags matter: converted without them, a decoder assumes a video
/// gamma and the export comes out looking more saturated than the
/// emulator did. `setparams` is used because `-color_*` output options
/// don't survive the filtergraph, and pinning the conversion matrix to
/// the one that gets tagged avoids a hue shift.
fn yuv_filter_chain(scale: u32, settings: &VideoSettings) -> String {
    let mut chain = String::new();
    if scale > 1 {
        chain.push_str(&format!("scale=iw*{scale}:ih*{scale}:flags=neighbor"));
    }
    let Some(color) = settings.color else {
        if !chain.is_empty() {
            chain.push(',');
        }
        chain.push_str("format=yuv420p");
        return chain;
    };
    if !chain.is_empty() {
        chain.push(':');
        chain.push_str(if color.full_range {
            "out_range=pc:out_color_matrix=bt709"
        } else {
            "out_range=tv:out_color_matrix=bt709"
        });
        chain.push(',');
    }
    chain.push_str("format=yuv420p,setparams=");
    chain.push_str(if color.full_range { "range=pc" } else { "range=tv" });
    chain.push_str(":colorspace=bt709:color_primaries=bt709:color_trc=iec61966-2-1");
    chain
}

fn audio_encode_args(settings: &AudioSettings) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    match settings.codec {
        AudioCodec::Aac { bitrate } => {
            args.extend(["-c:a", "aac"].map(String::from));
            args.extend(["-b:a".into(), bitrate.to_string()]);
        }
        AudioCodec::Flac => args.extend(["-c:a", "flac"].map(String::from)),
    }
    args.extend(["-ar".into(), settings.sample_rate.to_string()]);
    args.extend(["-ac".into(), settings.channels.to_string()]);
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ColorInfo;

    fn video_settings(quality: H264Quality, scale: u32) -> VideoSettings {
        VideoSettings {
            codec: VideoCodec::H264 { quality },
            width: 240,
            height: 160,
            scale,
            keyframe_interval: 120,
            timescale: 16_777_216,
            frame_duration: 280_896,
            color: Some(ColorInfo::SRGB_FULL),
        }
    }

    #[test]
    fn lossless_uses_the_rgb_encoder_and_no_color_conversion() {
        let args = video_encode_args(&video_settings(H264Quality::Lossless, 1)).join(" ");
        assert!(args.contains("libx264rgb"), "{args}");
        assert!(args.contains("-qp 0"), "{args}");
        assert!(!args.contains("yuv420p"), "lossless must not convert: {args}");
    }

    #[test]
    fn scaled_output_scales_in_ffmpeg_and_tags_full_range_srgb() {
        let args = video_encode_args(&video_settings(H264Quality::Crf(18), 3)).join(" ");
        assert!(args.contains("scale=iw*3:ih*3:flags=neighbor"), "{args}");
        assert!(args.contains("out_range=pc"), "{args}");
        assert!(args.contains("setparams=range=pc:colorspace=bt709"), "{args}");
        assert!(args.contains("color_trc=iec61966-2-1"), "{args}");
    }

    #[test]
    fn unscaled_lossy_output_still_converts_and_tags() {
        let args = video_encode_args(&video_settings(H264Quality::Crf(18), 1)).join(" ");
        assert!(!args.contains("scale="), "no scaling at 1x: {args}");
        assert!(args.contains("format=yuv420p,setparams=range=pc"), "{args}");
    }

    /// Nothing downstream carries a decode order distinct from the
    /// presentation one, so every video configuration must disable frame
    /// reordering.
    #[test]
    fn b_frames_are_always_off() {
        for quality in [H264Quality::Lossless, H264Quality::Crf(18), H264Quality::Bitrate(2_000_000)] {
            for scale in [1, 3] {
                let args = video_encode_args(&video_settings(quality, scale)).join(" ");
                assert!(args.contains("-bf 0"), "{quality:?} at {scale}x: {args}");
            }
        }
    }
}
