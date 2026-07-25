//! Replay video export: re-simulates a recorded replay through
//! [`tango_match::playback`] and feeds its frames and audio to
//! [`encoder_facade`], which encodes and muxes them into a video file.
//! Fully synchronous — the app runs it on a dedicated thread
//! ([`crate::app`]'s `spawn_replay_export`); the replays tab's inline
//! panel ([`crate::tabs::replays`]) owns the [`Canceller`] and renders
//! the progress callback.
//!
//! ffmpeg is only an *encoder* here: it is asked for bare elementary
//! streams (`-f h264`, `-f adts`, `-f ogg`) and the containers are
//! written in Rust. That means the bundled ffmpeg has to be built with
//! those output formats — `--enable-muxer=h264,adts,ogg` — and an
//! export that finds one without them says so before it starts.

use std::sync::Arc;

use image::EncodableLayout;

/// The cancel handle and the chapter list both belong to the encoder
/// now; the tab and the app still reach them through this module.
pub use encoder_facade::{Canceller, Chapter};

/// How an export should look. The scale doubles as the quality choice,
/// which is how the form presents it: one slider whose leftmost stop is
/// lossless.
pub struct Settings {
    /// `ffmpeg` binary to encode with. `None` looks beside the running
    /// executable and then on `PATH`.
    pub ffmpeg: Option<std::path::PathBuf>,
    /// `None` renders losslessly at native size; `Some(n)` renders an
    /// `n`-times nearest-neighbor upscale.
    pub scale: Option<usize>,
}

impl Settings {
    pub fn with_scale(scale: Option<usize>) -> Self {
        Self { ffmpeg: None, scale }
    }

    /// The container this export will write.
    pub fn container(&self) -> encoder_facade::Container {
        container(self.scale.is_none())
    }

    /// Translate into the encoder's settings. `width_screens` is the
    /// output width in GBA screens (1 one-sided, 2 side-by-side) and
    /// `audio_tracks` is one per screen.
    fn encoder_settings(&self, width_screens: u32, audio_tracks: usize) -> encoder_facade::Settings {
        let lossless = self.scale.is_none();
        encoder_facade::Settings {
            video: encoder_facade::VideoSettings {
                codec: encoder_facade::VideoCodec::H264,
                quality: if lossless {
                    encoder_facade::VideoQuality::Lossless
                } else {
                    encoder_facade::VideoQuality::Crf(18)
                },
                width: mgba::gba::SCREEN_WIDTH * width_screens,
                height: mgba::gba::SCREEN_HEIGHT,
                scale: self.scale.unwrap_or(1) as u32,
                keyframe_interval: KEYFRAME_INTERVAL,
                timescale: TIMESCALE,
                frame_duration: FRAME_DURATION,
                // A lossless render stays in RGB, and an RGB H.264
                // stream can't carry these at all.
                color: (!lossless).then_some(encoder_facade::ColorInfo::SRGB_FULL),
            },
            audio: encoder_facade::AudioSettings {
                codec: if lossless {
                    encoder_facade::AudioCodec::Flac
                } else {
                    encoder_facade::AudioCodec::Aac
                },
                sample_rate: SAMPLE_RATE as u32,
                channels: AUDIO_CHANNELS as u8,
                bitrate: 384_000,
            },
            container: self.container(),
            audio_tracks,
            // What the old pipeline's `-movflags +faststart` did: the
            // index goes in front of the media, so a player reading the
            // file in order can start before it has all of it. Matroska
            // ignores it.
            faststart: !lossless,
        }
    }
}

/// Which container a render lands in, and so which extension the save
/// dialog offers. Lossless is RGB H.264 with FLAC, neither of which MP4
/// carries, so it goes to Matroska.
pub fn container(lossless: bool) -> encoder_facade::Container {
    if lossless {
        encoder_facade::Container::Matroska
    } else {
        encoder_facade::Container::Mp4
    }
}

const SAMPLE_RATE: f64 = 48000.0;

const AUDIO_CHANNELS: usize = 2;

/// The GBA frame clock: 280896 cycles at 2^24 Hz, stated as a timebase
/// so the encoder can time frames exactly rather than in rounded
/// milliseconds.
const TIMESCALE: u32 = 16_777_216;
const FRAME_DURATION: u64 = 280_896;

/// Frames between keyframes — about half a second, which is what the
/// old flag string forced and fine granularity for scrubbing a replay.
const KEYFRAME_INTERVAL: u32 = 30;

/// The tick window an export writes, plus everything positional that
/// goes with it. A whole-replay export is just the degenerate clip
/// `0..=total` with no snapshot.
#[derive(Clone)]
pub struct Clip {
    /// First / last playhead tick whose frames are written, inclusive
    /// (the same coordinates as the player's readout).
    pub start: u32,
    pub end: u32,
    /// A whole-pair savestate at a tick strictly before `start` (from
    /// the player's keyframe store) to jump-start the re-sim at
    /// instead of simulating from boot. Without one the prefix
    /// simulates unwritten, same as a deselected round.
    ///
    /// A savestate restore replaces the priming-time pokes, so callers
    /// wanting BGM muted must pass `None` and eat the full re-sim.
    pub snapshot: Option<Arc<tango_match::playback::Snapshot>>,
    /// Inter-round transition ticks ([`tango_replay::Replay`]'s
    /// `round_starts` minus the leading 0, or the player's discovered
    /// boundaries for recordings that predate the markers). The round
    /// ordinal at any tick — for `rounds_mask` indexing and chapter
    /// titles — is the count of marks at or before it; a jump-started
    /// pair couldn't answer that from live telemetry, so no export
    /// runs any.
    pub round_marks: Vec<u32>,
}

impl std::fmt::Debug for Clip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Clip")
            .field("start", &self.start)
            .field("end", &self.end)
            .field("snapshot_tick", &self.snapshot.as_ref().map(|s| s.tick))
            .field("round_marks", &self.round_marks)
            .finish()
    }
}

/// Export an SIO replay ([`tango_replay::VERSION`]): one linear
/// pair re-sim produces both perspectives at once, so the two-sided
/// layout is a compose of the two framebuffers rather than a second
/// simulation. A tick reaches the encoders when it's inside `clip`'s
/// span AND its round is selected in `rounds_mask` (indices are
/// `clip.round_marks` intervals — the same ordering as the tab's
/// round checkboxes, which come from the same file marks). Unwritten
/// spans still simulate; they just aren't written.
///
/// Each written round becomes a chapter in the output container,
/// titled from `round_titles` (indexed like `rounds_mask`; falls back
/// to "Round N" past the end).
#[allow(clippy::too_many_arguments)]
pub fn export(
    config: &tango_match::playback::BootConfig,
    inputs: &[[u32; 2]],
    local_player: usize,
    rounds_mask: &[bool],
    round_titles: &[String],
    clip: &Clip,
    output_path: &std::path::Path,
    settings: &Settings,
    canceller: &Canceller,
    progress_callback: impl Fn(usize, usize),
    twosided: bool,
) -> anyhow::Result<()> {
    canceller.check()?;
    anyhow::ensure!(local_player < 2, "bad local player index");
    let last_selected = rounds_mask.iter().rposition(|&s| s);

    let (w, h) = (mgba::gba::SCREEN_WIDTH, mgba::gba::SCREEN_HEIGHT);
    let mut vbuf = image::RgbaImage::new(w, h);
    let mut composed_vbuf = image::RgbaImage::new(w * 2, h);
    let (width_screens, audio_tracks) = if twosided { (2, 2) } else { (1, 1) };

    // Encoders first: a bad ffmpeg or an impossible codec pairing should
    // fail before a re-simulation runs.
    let encoder_settings = settings.encoder_settings(width_screens, audio_tracks);
    let backend = encoder_facade::FfmpegBackend::new(&encoder_settings, settings.ffmpeg.clone(), canceller)?;
    let mut session = encoder_facade::Session::new(backend, encoder_settings)?;
    // Opened for reading as well: a faststart MP4 relocates its index,
    // which moves the media that follows it.
    let mut output = encoder_facade::Output::new(
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(output_path)?,
    );

    // Boot + prime. This is encoder-free but bounded (~a few hundred
    // ticks), so a cancel lands at the next loop check.
    let lifecycle = tango_match::telemetry::LifecycleSink::new();
    let mut pb = tango_match::playback::Playback::new(config, Arc::new(inputs.to_vec()), &lifecycle)?;
    // Drop the audio priming piled up (nothing drained during boot).
    for i in 0..2 {
        pb.pair_mut().core_mut(i).audio_buffer().clear();
    }

    // Jump-start a clip from its snapshot: the pair skips straight to
    // the capture tick and only the (≤ one keyframe interval) gap to
    // the span start simulates unwritten.
    if let Some(snap) = clip.snapshot.as_deref() {
        if snap.tick < clip.start {
            pb.load(snap)?;
            for i in 0..2 {
                pb.pair_mut().core_mut(i).audio_buffer().clear();
            }
        }
    }

    let mut samples = vec![0i16; SAMPLE_RATE as usize];
    let mut resamplers = [mgba::audio::AudioResampler::new(), mgba::audio::AudioResampler::new()];
    let mut dest_buffers = [
        mgba::audio::OwnedAudioBuffer::new(0x4000, AUDIO_CHANNELS as u32),
        mgba::audio::OwnedAudioBuffer::new(0x4000, AUDIO_CHANNELS as u32),
    ];
    let mut prev_should_write = false;
    // Chapter bookkeeping, in output frames: the open chapter's
    // (round, first written frame), closed when the written round
    // changes or a mask gap stops writing.
    let mut frames_written = 0u64;
    let mut chapters: Vec<Chapter> = vec![];
    let mut open_chapter: Option<(usize, u64)> = None;
    let close_chapter = |chapters: &mut Vec<Chapter>, (round, start): (usize, u64), end: u64| {
        if end > start {
            let title = round_titles
                .get(round)
                .cloned()
                .unwrap_or_else(|| format!("Round {}", round + 1));
            chapters.push(Chapter {
                title,
                start_frame: start,
                end_frame: end,
            });
        }
    };

    // Progress is relative to where the re-sim actually starts (the
    // snapshot tick for a jump-started clip) and ends (the span end),
    // so the bar doesn't sit near-done from tick one.
    let progress_base = pb.cursor() as usize;
    let progress_total = (clip.end as usize)
        .min(pb.total() as usize)
        .saturating_sub(progress_base);
    while pb.step() {
        canceller.check()?;
        let tick = pb.cursor();
        let cur_round = clip.round_marks.partition_point(|&m| m <= tick);
        if tick > clip.end || last_selected.is_none_or(|last| cur_round > last) {
            break;
        }

        let should_write = tick >= clip.start && rounds_mask.get(cur_round).copied().unwrap_or(false);
        if let Some(open) = open_chapter {
            if !should_write || open.0 != cur_round {
                close_chapter(&mut chapters, open, frames_written);
                open_chapter = None;
            }
        }
        if should_write && open_chapter.is_none() {
            open_chapter = Some((cur_round, frames_written));
        }
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
                let core = pair.core_mut(core_idx);
                let core_rate = core.audio_sample_rate() as f64;
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
                session.write_audio(slot, &samples[..n * AUDIO_CHANNELS])?;
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
                session.write_video(composed_vbuf.as_bytes())?;
            } else {
                session.write_video(vbuf.as_bytes())?;
            }
            frames_written += 1;
        }
        // Whatever the encoders have finished goes to the file as the
        // export runs, so memory stays flat however long the replay is.
        output.append(&session.take_output())?;
        progress_callback((tick as usize).saturating_sub(progress_base), progress_total);
    }
    if let Some(open) = open_chapter {
        close_chapter(&mut chapters, open, frames_written);
    }

    session.begin_finish()?;
    let fixups = loop {
        canceller.check()?;
        if let Some(fixups) = session.poll_finish(&chapters)? {
            break fixups;
        }
    };
    output.append(&session.take_output())?;
    output.finish(&fixups)?;
    Ok(())
}
