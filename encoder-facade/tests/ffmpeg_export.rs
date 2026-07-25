//! End-to-end exports through real ffmpeg, checked with real ffprobe.
//!
//! The unit tests cover the muxers against synthetic packets; these
//! cover the part that can only be checked against the real thing —
//! that ffmpeg's elementary streams parse, that the containers we build
//! from them satisfy libavformat, and that the audio comes out where the
//! video expects it.
//!
//! Set `ENCODER_FACADE_TEST_FFMPEG` to an ffmpeg binary to run them;
//! without it they skip, since not every machine has one and the sidecar
//! ffmpeg shipped with the app is a reduced build. `ffprobe` is looked
//! for beside it.
//!
//! ```text
//! ENCODER_FACADE_TEST_FFMPEG=/path/to/ffmpeg cargo test -p encoder-facade --test ffmpeg_export
//! ```

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use encoder_facade::{
    AudioCodec, AudioSettings, Canceller, Chapter, Container, FfmpegBackend, Output, Session, Settings, VideoCodec,
    VideoQuality, VideoSettings,
};

const WIDTH: u32 = 240;
const HEIGHT: u32 = 160;
/// The GBA frame clock: 280896 cycles at 2^24 Hz.
const TIMESCALE: u32 = 16_777_216;
const FRAME_DURATION: u64 = 280_896;
const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: u8 = 2;
/// Two seconds of video, enough for several clusters and a keyframe or
/// two without making the tests slow.
const FRAMES: u64 = 120;

fn ffmpeg() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os("ENCODER_FACADE_TEST_FFMPEG")?);
    path.exists().then_some(path)
}

/// `ffprobe` beside the ffmpeg under test.
fn ffprobe(ffmpeg: &Path) -> Option<PathBuf> {
    let mut probe = ffmpeg.with_file_name("ffprobe");
    probe.set_extension(std::env::consts::EXE_EXTENSION);
    probe.exists().then_some(probe)
}

macro_rules! require_ffmpeg {
    () => {
        match ffmpeg() {
            Some(path) => path,
            None => {
                eprintln!("skipping: set ENCODER_FACADE_TEST_FFMPEG to an ffmpeg binary");
                return;
            }
        }
    };
}

fn settings(container: Container, video: VideoCodec, audio: AudioCodec, quality: VideoQuality) -> Settings {
    Settings {
        video: VideoSettings {
            codec: video,
            quality,
            width: WIDTH,
            height: HEIGHT,
            scale: if quality == VideoQuality::Lossless { 1 } else { 2 },
            keyframe_interval: 60,
            timescale: TIMESCALE,
            frame_duration: FRAME_DURATION,
            color: (quality != VideoQuality::Lossless).then_some(encoder_facade::ColorInfo::SRGB_FULL),
        },
        audio: AudioSettings {
            codec: audio,
            sample_rate: SAMPLE_RATE,
            channels: CHANNELS,
            bitrate: 192_000,
        },
        container,
        audio_tracks: 1,
    }
}

/// A frame that changes every tick, so the encoder has real work to do
/// rather than encoding a run of identical frames.
fn frame(index: u64) -> Vec<u8> {
    let mut rgba = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let at = ((y * WIDTH + x) * 4) as usize;
            rgba[at] = (x as u64 + index * 3) as u8;
            rgba[at + 1] = (y as u64 + index * 5) as u8;
            rgba[at + 2] = (x + y) as u8;
            rgba[at + 3] = 0xff;
        }
    }
    rgba
}

/// Audio that is silent for the first half and loud for the second. The
/// step is what the A/V alignment check looks for: an encoder's priming
/// samples, if a container fails to declare them, push that step later
/// by the length of the priming.
fn samples_for_frame(index: u64) -> Vec<i16> {
    let per_frame = (SAMPLE_RATE as u64 * FRAME_DURATION) / TIMESCALE as u64;
    let start = index * per_frame;
    let mut out = Vec::with_capacity((per_frame * CHANNELS as u64) as usize);
    for i in 0..per_frame {
        let absolute = start + i;
        let value = if absolute >= SILENT_SAMPLES {
            // A square wave: loud, and unmistakable after lossy coding.
            if (absolute / 24).is_multiple_of(2) {
                12_000
            } else {
                -12_000
            }
        } else {
            0
        };
        out.push(value);
        out.push(value);
    }
    out
}

/// Samples of silence before the step.
const SILENT_SAMPLES: u64 = 24_000;

/// Run a whole export and return the file it wrote.
fn export(ffmpeg: &Path, settings: Settings, chapters: &[Chapter], name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("encoder-facade-{name}.{}", settings.container.extension()));
    let file = std::fs::File::create(&path).expect("create the output file");
    let mut output = Output::new(file);

    let canceller = Canceller::new();
    let backend =
        FfmpegBackend::new(&settings, Some(ffmpeg.to_path_buf()), &canceller).expect("spawn the encoders");
    let mut session = Session::new(backend, settings).expect("open the session");

    for index in 0..FRAMES {
        session.write_video(&frame(index)).expect("write a frame");
        session.write_audio(0, &samples_for_frame(index)).expect("write samples");
        output.append(&session.take_output()).expect("append output");
    }
    session.begin_finish().expect("begin finishing");
    let patches = loop {
        if let Some(patches) = session.poll_finish(chapters).expect("finish") {
            break patches;
        }
    };
    output.append(&session.take_output()).expect("append the tail");
    output.finish(&patches).expect("apply the patches");
    path
}

/// `ffprobe -show_streams -show_chapters` output as flat `key=value`
/// lines, or `None` if there's no ffprobe to ask.
fn probe(ffmpeg: &Path, file: &Path) -> Option<String> {
    let ffprobe = ffprobe(ffmpeg)?;
    let output = Command::new(ffprobe)
        .args(["-hide_banner", "-loglevel", "error"])
        .args(["-show_streams", "-show_chapters", "-show_format", "-of", "flat"])
        .arg(file)
        .output()
        .expect("run ffprobe");
    assert!(
        output.status.success(),
        "ffprobe rejected {}: {}",
        file.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Decode a file's audio back to interleaved 16-bit samples.
fn decode_audio(ffmpeg: &Path, file: &Path) -> Vec<i16> {
    let output = Command::new(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(file)
        .args(["-map", "0:a:0", "-f", "s16le", "-acodec", "pcm_s16le", "-"])
        .output()
        .expect("run ffmpeg to decode");
    assert!(
        output.status.success(),
        "ffmpeg couldn't decode {}: {}",
        file.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    output
        .stdout
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect()
}

/// First sample (per channel) whose level clears `threshold`.
fn first_loud_sample(samples: &[i16]) -> Option<u64> {
    samples
        .chunks_exact(CHANNELS as usize)
        .position(|frame| frame.iter().any(|s| s.unsigned_abs() > 4_000))
        .map(|p| p as u64)
}

#[test]
fn mp4_h264_aac_is_what_ffprobe_expects() {
    let ffmpeg = require_ffmpeg!();
    let file = export(
        &ffmpeg,
        settings(Container::Mp4, VideoCodec::H264, AudioCodec::Aac, VideoQuality::Crf(20)),
        &[],
        "h264-aac",
    );
    let Some(probed) = probe(&ffmpeg, &file) else {
        eprintln!("no ffprobe beside ffmpeg; the file was written but not probed");
        return;
    };
    assert!(probed.contains("codec_name=\"h264\""), "{probed}");
    assert!(probed.contains("codec_tag_string=\"avc1\""), "{probed}");
    assert!(probed.contains("codec_name=\"aac\""), "{probed}");
    assert!(probed.contains("codec_tag_string=\"mp4a\""), "{probed}");
    // The scale factor is applied by the encoder, so the file is twice
    // the frame size it was fed.
    assert!(probed.contains("width=480"), "{probed}");
    assert!(probed.contains("height=320"), "{probed}");
    assert!(
        probed.contains(&format!("nb_frames=\"{FRAMES}\"")),
        "every frame pushed must be in the file: {probed}"
    );
    // 120 frames of the GBA clock is 2.0086 seconds.
    assert!(probed.contains("duration=\"2.0"), "{probed}");
}

#[test]
fn matroska_lossless_h264_flac_is_what_ffprobe_expects() {
    let ffmpeg = require_ffmpeg!();
    let file = export(
        &ffmpeg,
        settings(
            Container::Matroska,
            VideoCodec::H264,
            AudioCodec::Flac,
            VideoQuality::Lossless,
        ),
        &[],
        "lossless",
    );
    let Some(probed) = probe(&ffmpeg, &file) else {
        return;
    };
    assert!(probed.contains("codec_name=\"h264\""), "{probed}");
    assert!(probed.contains("codec_name=\"flac\""), "{probed}");
    // Lossless keeps the input size.
    assert!(probed.contains("width=240"), "{probed}");
    assert!(probed.contains("height=160"), "{probed}");
}

#[test]
fn webm_vp9_opus_is_what_ffprobe_expects() {
    let ffmpeg = require_ffmpeg!();
    let file = export(
        &ffmpeg,
        settings(
            Container::WebM,
            VideoCodec::Vp9,
            AudioCodec::Opus,
            VideoQuality::Bitrate(2_000_000),
        ),
        &[],
        "vp9-opus",
    );
    let Some(probed) = probe(&ffmpeg, &file) else {
        return;
    };
    assert!(probed.contains("codec_name=\"vp9\""), "{probed}");
    assert!(probed.contains("codec_name=\"opus\""), "{probed}");
}

/// Chapters are the reason this crate assembles its own containers
/// rather than handing packets to a simpler writer, so they get checked
/// in both containers that carry them.
#[test]
fn chapters_come_back_out_of_both_containers() {
    let ffmpeg = require_ffmpeg!();
    let chapters = vec![
        Chapter {
            title: "Round 1".into(),
            start_frame: 0,
            end_frame: 60,
        },
        Chapter {
            title: "Round 2".into(),
            start_frame: 60,
            end_frame: FRAMES,
        },
    ];
    for (container, video, audio, quality, name) in [
        (
            Container::Mp4,
            VideoCodec::H264,
            AudioCodec::Aac,
            VideoQuality::Crf(24),
            "chapters-mp4",
        ),
        (
            Container::Matroska,
            VideoCodec::H264,
            AudioCodec::Aac,
            VideoQuality::Crf(24),
            "chapters-mkv",
        ),
    ] {
        let file = export(&ffmpeg, settings(container, video, audio, quality), &chapters, name);
        let Some(probed) = probe(&ffmpeg, &file) else {
            return;
        };
        assert!(
            probed.contains("Round 1") && probed.contains("Round 2"),
            "{container:?} lost its chapter titles: {probed}"
        );
        // Two chapters, the second starting one second in (60 frames of
        // the GBA clock is 1.0043 s).
        assert!(probed.contains("chapters.chapter.1"), "{container:?}: {probed}");
        assert!(
            probed.contains("start_time=\"1.00"),
            "{container:?} put the second chapter in the wrong place: {probed}"
        );
    }
}

/// The check that a container's audio lands where its video expects it.
///
/// The input is silent for exactly half a second and then loud. A lossy
/// encoder emits priming samples before the real audio, and a container
/// that doesn't declare them (MP4 edit list, Matroska `CodecDelay`)
/// plays everything that much late — 21 ms for AAC, which is audible as
/// lip-sync error and would show up here as the step arriving ~1024
/// samples late.
#[test]
fn audio_is_not_shifted_by_encoder_priming() {
    let ffmpeg = require_ffmpeg!();
    for (container, name) in [(Container::Mp4, "sync-mp4"), (Container::Matroska, "sync-mkv")] {
        let file = export(
            &ffmpeg,
            settings(container, VideoCodec::H264, AudioCodec::Aac, VideoQuality::Crf(24)),
            &[],
            name,
        );
        let decoded = decode_audio(&ffmpeg, &file);
        let step = first_loud_sample(&decoded).expect("the decoded audio must have a loud half");
        let drift = step as i64 - SILENT_SAMPLES as i64;
        assert!(
            drift.abs() <= 256,
            "{container:?}: the step landed {drift} samples from where it was written \
             (AAC priming is 1024 samples, so a drift near that means the delay isn't declared)"
        );
    }
}

/// Lossless means lossless: FLAC in Matroska has to decode back to the
/// exact samples that went in.
#[test]
fn lossless_audio_survives_bit_exact() {
    let ffmpeg = require_ffmpeg!();
    let file = export(
        &ffmpeg,
        settings(
            Container::Matroska,
            VideoCodec::H264,
            AudioCodec::Flac,
            VideoQuality::Lossless,
        ),
        &[],
        "flac-exact",
    );
    let decoded = decode_audio(&ffmpeg, &file);
    let expected: Vec<i16> = (0..FRAMES).flat_map(samples_for_frame).collect();
    assert!(
        decoded.len() >= expected.len(),
        "decoded {} samples, expected at least {}",
        decoded.len(),
        expected.len()
    );
    assert_eq!(
        &decoded[..expected.len()],
        &expected[..],
        "FLAC in Matroska must round-trip exactly"
    );
}

/// libavformat has to accept what we wrote well enough to copy the
/// streams out of it — the check that the container is structurally
/// sound rather than merely probeable.
#[test]
fn ffmpeg_can_remux_our_output() {
    let ffmpeg = require_ffmpeg!();
    for (container, name) in [
        (Container::Mp4, "remux-mp4"),
        (Container::Matroska, "remux-mkv"),
        (Container::WebM, "remux-webm"),
    ] {
        let (video, audio, quality) = if container == Container::WebM {
            (VideoCodec::Vp9, AudioCodec::Opus, VideoQuality::Bitrate(1_500_000))
        } else {
            (VideoCodec::H264, AudioCodec::Aac, VideoQuality::Crf(26))
        };
        let file = export(&ffmpeg, settings(container, video, audio, quality), &[], name);
        let out = file.with_extension("remuxed.mkv");
        let result = Command::new(&ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
            .arg(&file)
            .args(["-c", "copy"])
            .arg(&out)
            .output()
            .expect("run ffmpeg");
        assert!(
            result.status.success(),
            "ffmpeg couldn't remux our {container:?}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let _ = std::fs::remove_file(out);
    }
}

/// The shape a side-by-side replay export has: a double-width frame and
/// one audio track per side, with chapters at the round boundaries.
/// Multiple audio tracks are the part no other test here covers.
#[test]
fn a_two_sided_export_carries_a_track_per_side() {
    let ffmpeg = require_ffmpeg!();
    let mut settings = settings(Container::Mp4, VideoCodec::H264, AudioCodec::Aac, VideoQuality::Crf(24));
    settings.video.width = WIDTH * 2;
    settings.audio_tracks = 2;

    let path = std::env::temp_dir().join("encoder-facade-two-sided.mp4");
    let mut output = Output::new(std::fs::File::create(&path).expect("create the output"));
    let canceller = Canceller::new();
    let backend = FfmpegBackend::new(&settings, Some(ffmpeg.clone()), &canceller).expect("spawn the encoders");
    let frame_bytes = settings.video.frame_bytes();
    let mut session = Session::new(backend, settings).expect("open the session");

    for index in 0..FRAMES {
        // Two screens side by side, each the single-screen frame.
        let single = frame(index);
        let mut composed = vec![0u8; frame_bytes];
        for row in 0..HEIGHT as usize {
            let width = WIDTH as usize * 4;
            let from = row * width;
            let to = row * width * 2;
            composed[to..to + width].copy_from_slice(&single[from..from + width]);
            composed[to + width..to + width * 2].copy_from_slice(&single[from..from + width]);
        }
        session.write_video(&composed).expect("write a frame");
        // One side's audio, the other side's shifted, so a muxer that
        // mixed the tracks up would be visible in the result.
        let samples = samples_for_frame(index);
        session.write_audio(0, &samples).expect("write side one");
        session.write_audio(1, &samples).expect("write side two");
        output.append(&session.take_output()).expect("append output");
    }
    session.begin_finish().expect("begin finishing");
    let patches = loop {
        if let Some(patches) = session.poll_finish(&[]).expect("finish") {
            break patches;
        }
    };
    output.append(&session.take_output()).expect("append the tail");
    output.finish(&patches).expect("apply the patches");

    let Some(probed) = probe(&ffmpeg, &path) else {
        return;
    };
    assert_eq!(
        probed.matches("codec_name=\"aac\"").count(),
        2,
        "both sides' audio tracks must be present: {probed}"
    );
    assert!(probed.contains("width=960"), "two screens at 2x: {probed}");
    // Both tracks must be full length, not one of them truncated.
    assert!(
        probed.matches("duration=\"2.0").count() >= 3,
        "video and both audio tracks should run the same length: {probed}"
    );
}

/// A build without the raw output formats has to say so, rather than
/// failing somewhere deep in an export.
#[test]
fn a_build_without_elementary_formats_is_reported_clearly() {
    let ffmpeg = require_ffmpeg!();
    let mut settings = settings(Container::Mp4, VideoCodec::H264, AudioCodec::Aac, VideoQuality::Crf(24));
    settings.audio_tracks = 1;
    // A binary that exists but is not ffmpeg at all.
    let mut fake = std::env::temp_dir().join("encoder-facade-not-ffmpeg");
    fake.set_extension(std::env::consts::EXE_EXTENSION);
    std::fs::File::create(&fake)
        .and_then(|mut f| f.write_all(b"not an executable"))
        .expect("write the stand-in");
    let message = match FfmpegBackend::new(&settings, Some(fake.clone()), &Canceller::new()) {
        Ok(_) => panic!("a non-ffmpeg binary must not produce a working backend"),
        Err(error) => error.to_string(),
    };
    assert!(
        message.contains("ffmpeg") || message.contains("output format"),
        "the error should name ffmpeg or the missing formats: {message}"
    );
    let _ = std::fs::remove_file(fake);
    let _ = ffmpeg;
}
