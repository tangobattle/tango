//! The native encoder backend: one ffmpeg subprocess per stream.
//!
//! Each child reads raw frames or samples on its stdin and writes a bare
//! elementary stream on its stdout — no container, no muxing. A reader
//! thread drains that stdout continuously (a process can deadlock
//! against its own pipe if the writer waits for the reader while the
//! reader waits for the writer), and the export thread turns those bytes
//! into packets through [`crate::es`].
//!
//! Two constraints shape the ffmpeg command lines:
//!
//!   * **No B-frames** (`-bf 0`). An elementary stream carries no
//!     timestamps, so packets are timed by their ordinal; that's only
//!     correct while output order equals input order, which frame
//!     reordering would break. The cost is a few percent of bitrate.
//!   * **Encoder-side scaling.** Pre-scaled frames would mean pushing
//!     the upscaled bytes through a pipe — at 10× that is hundreds of
//!     megabytes a second — so the scale filter runs in ffmpeg and the
//!     caller keeps pushing frames at their native size.

use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use crate::cancel::ChildSlot;
use crate::es::{self, EsParser};
use crate::{
    AudioCodec, AudioSettings, Backend, Canceller, Error, Packet, Settings, VideoCodec, VideoQuality, VideoSettings,
};

/// ffmpeg's native AAC encoder primes with one full frame, which a
/// player has to discard to stay in sync with the video. It isn't
/// recoverable from an ADTS stream, so it's stated here — the one place
/// that knows which encoder produced the stream.
const AAC_ENCODER_DELAY_SAMPLES: u64 = 1024;

/// Keep the last of a child's stderr for error messages. A failing
/// ffmpeg explains itself there and nowhere else, and in a GUI build
/// nobody sees the console.
const STDERR_TAIL_LINES: usize = 12;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

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

/// Check that this ffmpeg can write the elementary-stream formats an
/// export needs.
///
/// Worth doing up front because a build trimmed to muxing duties — as a
/// bundled sidecar tends to be — can have every encoder we ask for and
/// still lack the raw output formats, and ffmpeg's own complaint
/// ("Requested output format 'h264' is not known") doesn't say that the
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
    // Lines look like "  E  h264   raw H.264 video"; take the name
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

/// One encoder subprocess and the parser for what it produces.
pub struct Encoder {
    slot: ChildSlot,
    stdin: Option<std::process::ChildStdin>,
    stdout: Receiver<Vec<u8>>,
    stderr: Arc<Mutex<Vec<String>>>,
    reader: Option<std::thread::JoinHandle<()>>,
    parser: Box<dyn EsParser>,
    /// Track ticks per unit of the parser's reported durations: a frame
    /// duration for video, 1 for audio (whose unit is already a sample).
    ticks_per_unit: u64,
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
        command.args(["-f", video_format(settings.codec), "pipe:1"]);
        Self::spawn(
            command,
            canceller,
            es::for_video(settings.codec),
            settings.frame_duration,
        )
    }

    /// Spawn one audio encoder. Returns `None` for PCM, which needs no
    /// encoder at all.
    pub fn audio(ffmpeg: &Path, settings: &AudioSettings, canceller: &Canceller) -> crate::Result<Option<Self>> {
        let Some(parser) = es::for_audio(settings.codec) else {
            return Ok(None);
        };
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
        command.args(["-f", audio_format(settings.codec), "pipe:1"]);
        Self::spawn(command, canceller, parser, 1).map(Some)
    }

    fn spawn(
        mut command: Command,
        canceller: &Canceller,
        parser: Box<dyn EsParser>,
        ticks_per_unit: u64,
    ) -> crate::Result<Self> {
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
            parser,
            ticks_per_unit,
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
        while let Ok(chunk) = self.stdout.try_recv() {
            self.parser.push(&chunk).map_err(|e| self.error(e.to_string()))?;
        }
        Ok(self.drain_parser())
    }

    /// Codec configuration, once the stream has revealed it.
    pub fn codec_private(&self) -> Option<Vec<u8>> {
        self.parser.codec_private()
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
        while let Ok(chunk) = self.stdout.try_recv() {
            self.parser.push(&chunk).map_err(|e| self.error(e.to_string()))?;
        }
        self.parser.flush().map_err(|e| self.error(e.to_string()))?;
        let packets = self.drain_parser();

        let taken = self.slot.lock().unwrap().take();
        let mut child = taken.ok_or_else(|| Error::internal("the encoder was already reaped"))?;
        let status = child.wait()?;
        if !status.success() {
            return Err(self.error(format!("ffmpeg exited with {status}")));
        }
        Ok(packets)
    }

    /// Encoder priming this stream needs a player to discard, in
    /// samples: whatever the stream declared, else what we know about
    /// the encoder we asked for.
    pub fn codec_delay_samples(&self, codec: AudioCodec) -> u64 {
        self.parser.declared_codec_delay().unwrap_or(match codec {
            AudioCodec::Aac => AAC_ENCODER_DELAY_SAMPLES,
            _ => 0,
        })
    }

    fn drain_parser(&mut self) -> Vec<Packet> {
        self.parser
            .take_packets()
            .into_iter()
            .map(|unit| {
                let duration = unit.duration * self.ticks_per_unit;
                let pts = self.next_pts;
                self.next_pts += duration;
                Packet {
                    pts,
                    duration,
                    keyframe: unit.keyframe,
                    data: unit.data,
                }
            })
            .collect()
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
    /// One per audio track. `None` where the codec is PCM — those
    /// samples are already packets and never see an encoder.
    audio: Vec<Option<Encoder>>,
    audio_codec: AudioCodec,
    channels: u8,
    /// Running sample counts for PCM tracks, which are timed here rather
    /// than by an encoder.
    pcm_pts: Vec<u64>,
    /// Packets produced outside a `poll` — PCM passthrough, and the
    /// tails collected while flushing.
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
        check_formats(&path, &required_formats(settings.video.codec, settings.audio.codec))?;

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
            channels: settings.audio.channels,
            pcm_pts: vec![0; settings.audio_tracks],
            ready: Vec::new(),
            // Video plus one per audio track.
            flush_done: vec![false; settings.audio_tracks + 1],
        })
    }

    fn encoder(&mut self, track: usize) -> crate::Result<Option<&mut Encoder>> {
        if track == crate::VIDEO_TRACK {
            return Ok(Some(&mut self.video));
        }
        match self.audio.get_mut(track - 1) {
            Some(slot) => Ok(slot.as_mut()),
            None => Err(Error::internal(format!("no track {track}"))),
        }
    }
}

impl Backend for FfmpegBackend {
    fn submit_video(&mut self, frame: &[u8]) -> crate::Result<()> {
        self.video.write(frame)
    }

    fn submit_audio(&mut self, track: usize, samples: &[i16]) -> crate::Result<()> {
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        match self.encoder(track + 1)? {
            Some(encoder) => encoder.write(&bytes),
            None => {
                // PCM: the samples are the packet, so this is where they
                // get timed.
                let frames = samples.len() as u64 / self.channels.max(1) as u64;
                if frames > 0 {
                    let pts = self.pcm_pts[track];
                    self.pcm_pts[track] += frames;
                    self.ready.push((
                        track + 1,
                        Packet {
                            pts,
                            duration: frames,
                            keyframe: true,
                            data: bytes,
                        },
                    ));
                }
                Ok(())
            }
        }
    }

    fn poll(&mut self) -> crate::Result<Vec<(usize, Packet)>> {
        let mut out = std::mem::take(&mut self.ready);
        for packet in self.video.poll()? {
            out.push((crate::VIDEO_TRACK, packet));
        }
        for track in 0..self.audio.len() {
            if let Some(encoder) = self.audio[track].as_mut() {
                for packet in encoder.poll()? {
                    out.push((track + 1, packet));
                }
            }
        }
        Ok(out)
    }

    fn codec_private(&self, track: usize) -> Option<Vec<u8>> {
        if track == crate::VIDEO_TRACK {
            return self.video.codec_private();
        }
        match self.audio.get(track - 1) {
            // PCM describes itself entirely through the track header.
            Some(None) => Some(Vec::new()),
            Some(Some(encoder)) => encoder.codec_private(),
            None => None,
        }
    }

    fn codec_delay_samples(&self, track: usize) -> u64 {
        if track == crate::VIDEO_TRACK {
            return 0;
        }
        match self.audio.get(track - 1) {
            Some(Some(encoder)) => encoder.codec_delay_samples(self.audio_codec),
            _ => 0,
        }
    }

    fn begin_flush(&mut self) -> crate::Result<()> {
        self.video.close_input();
        for encoder in self.audio.iter_mut().flatten() {
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
            if let Some(encoder) = self.audio[track].as_mut() {
                let tail = encoder.finish()?;
                self.ready.extend(tail.into_iter().map(|p| (track + 1, p)));
            }
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

/// The elementary-stream output format for each codec — the framing
/// [`crate::es`] knows how to read back.
fn video_format(codec: VideoCodec) -> &'static str {
    match codec {
        VideoCodec::H264 => "h264",
        VideoCodec::Vp8 | VideoCodec::Vp9 => "ivf",
    }
}

fn audio_format(codec: AudioCodec) -> &'static str {
    match codec {
        AudioCodec::Aac => "adts",
        // Ogg is the only framing ffmpeg will write for these that
        // preserves packet boundaries.
        AudioCodec::Opus => "opus",
        AudioCodec::Flac => "ogg",
        AudioCodec::PcmS16Le => "s16le",
    }
}

/// Every output format an export will ask this ffmpeg for.
pub fn required_formats(video: VideoCodec, audio: AudioCodec) -> Vec<&'static str> {
    let mut formats = vec![video_format(video)];
    if audio != AudioCodec::PcmS16Le {
        formats.push(audio_format(audio));
    }
    formats
}

fn video_encode_args(settings: &VideoSettings) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    let scale = settings.scale;
    match settings.codec {
        VideoCodec::H264 => match settings.quality {
            VideoQuality::Lossless => {
                // RGB in, RGB out: no color conversion, nothing to
                // lose. An RGB H.264 stream can't carry color tags at
                // all, which is why this path sets none.
                args.extend(["-c:v", "libx264rgb", "-preset", "ultrafast", "-qp", "0"].map(String::from));
                if scale > 1 {
                    args.extend(["-vf".into(), format!("scale=iw*{scale}:ih*{scale}:flags=neighbor")]);
                }
            }
            VideoQuality::Crf(crf) => {
                args.extend(["-c:v", "libx264"].map(String::from));
                args.extend(["-vf".into(), yuv_filter_chain(scale, settings)]);
                args.extend(["-crf".into(), crf.to_string()]);
            }
            VideoQuality::Bitrate(bits) => {
                args.extend(["-c:v", "libx264"].map(String::from));
                args.extend(["-vf".into(), yuv_filter_chain(scale, settings)]);
                args.extend(["-b:v".into(), bits.to_string()]);
            }
        },
        VideoCodec::Vp8 | VideoCodec::Vp9 => {
            let encoder = if settings.codec == VideoCodec::Vp8 {
                "libvpx"
            } else {
                "libvpx-vp9"
            };
            args.extend(["-c:v", encoder, "-deadline", "good", "-cpu-used", "4"].map(String::from));
            args.extend(["-vf".into(), yuv_filter_chain(scale, settings)]);
            let bits = match settings.quality {
                VideoQuality::Bitrate(bits) => bits,
                // libvpx wants a rate target; pick one rather than let
                // it fall back to its 256 kbit/s default.
                _ => 4_000_000,
            };
            args.extend(["-b:v".into(), bits.to_string()]);
        }
    }
    // Frame reordering would break ordinal timing; see the module docs.
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
        AudioCodec::Aac => {
            args.extend(["-c:a", "aac"].map(String::from));
            args.extend(["-b:a".into(), settings.bitrate.to_string()]);
        }
        AudioCodec::Opus => {
            args.extend(["-c:a", "libopus"].map(String::from));
            args.extend(["-b:a".into(), settings.bitrate.to_string()]);
        }
        AudioCodec::Flac => args.extend(["-c:a", "flac"].map(String::from)),
        AudioCodec::PcmS16Le => args.extend(["-c:a", "pcm_s16le"].map(String::from)),
    }
    args.extend(["-ar".into(), settings.sample_rate.to_string()]);
    args.extend(["-ac".into(), settings.channels.to_string()]);
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ColorInfo;

    fn video_settings(quality: VideoQuality, scale: u32) -> VideoSettings {
        VideoSettings {
            codec: VideoCodec::H264,
            quality,
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
        let args = video_encode_args(&video_settings(VideoQuality::Lossless, 1)).join(" ");
        assert!(args.contains("libx264rgb"), "{args}");
        assert!(args.contains("-qp 0"), "{args}");
        assert!(!args.contains("yuv420p"), "lossless must not convert: {args}");
    }

    #[test]
    fn scaled_output_scales_in_ffmpeg_and_tags_full_range_srgb() {
        let args = video_encode_args(&video_settings(VideoQuality::Crf(18), 3)).join(" ");
        assert!(args.contains("scale=iw*3:ih*3:flags=neighbor"), "{args}");
        assert!(args.contains("out_range=pc"), "{args}");
        assert!(args.contains("setparams=range=pc:colorspace=bt709"), "{args}");
        assert!(args.contains("color_trc=iec61966-2-1"), "{args}");
    }

    #[test]
    fn unscaled_lossy_output_still_converts_and_tags() {
        let args = video_encode_args(&video_settings(VideoQuality::Crf(18), 1)).join(" ");
        assert!(!args.contains("scale="), "no scaling at 1x: {args}");
        assert!(args.contains("format=yuv420p,setparams=range=pc"), "{args}");
    }

    /// Ordinal timing depends on it, so every video configuration must
    /// disable frame reordering.
    #[test]
    fn b_frames_are_always_off() {
        for quality in [VideoQuality::Lossless, VideoQuality::Crf(18), VideoQuality::Bitrate(2_000_000)] {
            for scale in [1, 3] {
                let args = video_encode_args(&video_settings(quality, scale)).join(" ");
                assert!(args.contains("-bf 0"), "{quality:?} at {scale}x: {args}");
            }
        }
    }

    #[test]
    fn formats_are_the_elementary_ones() {
        assert_eq!(required_formats(VideoCodec::H264, AudioCodec::Aac), vec!["h264", "adts"]);
        assert_eq!(required_formats(VideoCodec::Vp9, AudioCodec::Opus), vec!["ivf", "opus"]);
        assert_eq!(required_formats(VideoCodec::H264, AudioCodec::Flac), vec!["h264", "ogg"]);
        assert_eq!(
            required_formats(VideoCodec::H264, AudioCodec::PcmS16Le),
            vec!["h264"],
            "PCM needs no encoder, so it needs no output format"
        );
    }
}
