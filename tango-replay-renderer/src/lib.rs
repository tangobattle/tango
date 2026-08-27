//! Turning a recorded replay back into video: re-simulates it through
//! the seam's replay path ([`tango_match::ReplaySet`]) and feeds the
//! frames and audio to [`encoder_facade`], which muxes the raw
//! streams directly or encodes the shareable ones into a video file.
//!
//! Engine-neutral on purpose: the boot comes from the game's own
//! [`tango_match::Backend`] — the same registration the player watches
//! replays through — and everything read back (RGBA8 frames, interleaved
//! audio, the console's screen layout and frame clock) is the seam's,
//! so a GBA replay and a DS replay render through the same pipeline.
//!
//! Nothing here picks a backend or opens a file. [`encoder_facade`]
//! copies raw media itself, or starts whichever encoder the
//! target has (ffmpeg subprocesses natively, WebCodecs in a browser).
//! The host passes in something seekable to write to — a `File`, a
//! `Cursor<Vec<u8>>`, a shim over an OPFS sync handle — so the same
//! re-simulation serves both.
//!
//! A render is driven, not run: [`Render::pump`] advances a slice of
//! ticks and hands control back. A thread can pump until it's done —
//! that's [`render`], which is what the desktop app calls — while a
//! browser pumps from its event loop, which is the only way the
//! WebCodecs backend can work at all, since its encoders deliver their
//! packets through callbacks that a blocking loop would starve.
//! [`Progress`] says which phase a render is in and how far along, and
//! the [`Canceller`] stops it wherever it is.

use std::sync::Arc;

/// The cancel handle and the chapter list both belong to the encoder;
/// hosts reach them through this module.
pub use encoder_facade::{Canceller, Chapter};

/// What can go wrong rendering a replay.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Encoding, muxing, or writing the output. Carries
    /// [`encoder_facade::Error::Cancelled`] too — a killed
    /// [`Canceller`] ends the render as an error, and hosts tell that
    /// apart from a failure by asking the canceller.
    #[error(transparent)]
    Encoder(#[from] encoder_facade::Error),
    /// Booting, priming, or restoring the re-simulation pair.
    #[error(transparent)]
    Engine(#[from] tango_match::Error),
    /// A replay naming a side the two-seat pair doesn't have.
    #[error("bad local player index {0}")]
    BadLocalPlayer(usize),
    /// [`Render::pump`] called after it reported [`Progress::Done`].
    #[error("this render has already finished")]
    AlreadyFinished,
}

pub type Result<T> = std::result::Result<T, Error>;

/// What a render can write its container to: anything seekable, since
/// a finished container has fields that can only be filled in once the
/// stream ends. A `File` on a desktop, a `Cursor<Vec<u8>>` for an
/// render that hands the bytes to a browser download, a shim over an
/// OPFS sync access handle for one that streams to disk in a worker.
pub trait Writer: std::io::Read + std::io::Write + std::io::Seek {}

impl<W: std::io::Read + std::io::Write + std::io::Seek> Writer for W {}

/// Which container a render lands in, and so which extension a save
/// dialog should offer. Raw output is uncompressed RGB24 with PCM, which
/// goes to Matroska; the shareable export is H.264/AAC in MP4.
pub fn container(raw_output: bool) -> encoder_facade::Container {
    if raw_output {
        encoder_facade::Container::Matroska
    } else {
        encoder_facade::Container::Mp4
    }
}

/// Translate a render's choices into encoder settings. `width` and
/// `height` are the composed frame's native size; `timing` is the
/// console's exact frame clock
/// ([`tango_match::Backend::frame_timing`]).
fn encoder_settings(
    scale: Option<usize>,
    width: u32,
    height: u32,
    timing: tango_match::FrameTiming,
    audio_sample_rate: encoder_facade::SampleRate,
    audio_tracks: usize,
) -> encoder_facade::Settings {
    let raw_output = scale.is_none();
    encoder_facade::Settings {
        video: encoder_facade::VideoSettings {
            // The archival path copies RGB channels into Matroska
            // without an encoder. The shareable path is CRF-rated,
            // which at this frame size is predictable where a bitrate
            // target would swing with the content.
            codec: if raw_output {
                encoder_facade::VideoCodec::RawRgb24
            } else {
                encoder_facade::VideoCodec::H264 {
                    quality: encoder_facade::H264Quality::Crf(18),
                }
            },
            width,
            height,
            scale: scale.unwrap_or(1) as u32,
            keyframe_interval: KEYFRAME_INTERVAL,
            timescale: timing.timescale,
            frame_duration: timing.frame_duration,
            color: Some(if raw_output {
                encoder_facade::ColorInfo::SRGB_RGB_FULL
            } else {
                encoder_facade::ColorInfo::SRGB_FULL
            }),
        },
        audio: encoder_facade::AudioSettings {
            codec: if raw_output {
                encoder_facade::AudioCodec::PcmS16Le
            } else {
                encoder_facade::AudioCodec::Aac { bitrate: 384_000 }
            },
            sample_rate: audio_sample_rate,
            channels: AUDIO_CHANNELS as u8,
        },
        container: container(raw_output),
        audio_tracks,
        // What the old pipeline's `-movflags +faststart` did: the
        // index goes in front of the media, so a player reading the
        // file in order can start before it has all of it. Matroska
        // ignores it.
        faststart: !raw_output,
    }
}

/// AAC exports stay at the broadly-supported 48 kHz rate. Raw exports
/// use the rate reported by the emulator instead.
const LOSSY_SAMPLE_RATE: f64 = 48_000.0;

const AUDIO_CHANNELS: usize = 2;

/// Frames between keyframes — about half a second, which is what the
/// old flag string forced and fine granularity for scrubbing a replay.
const KEYFRAME_INTERVAL: u32 = 30;

/// How far ahead of its encoders a render may get before a pump slice
/// gives up its turn — half a second of frames. Only bites on a backend
/// that queues instead of blocking (WebCodecs); a blocking one never
/// reports a depth at all.
const MAX_ENCODER_QUEUE: u32 = 30;

/// Ticks one [`render`] pump slice runs before reporting progress. Small
/// enough that a progress bar moves several times a second, large enough
/// that the per-slice bookkeeping is noise next to the emulation.
const BLOCKING_SLICE_TICKS: usize = 8;

/// The tick window a render writes, plus everything positional that
/// goes with it. A whole-replay render is just the degenerate clip
/// `0..=total` with no snapshot.
#[derive(Clone)]
pub struct Clip {
    /// First / last playhead tick whose frames are written, inclusive
    /// (the same coordinates as the player's readout).
    pub start: u32,
    pub end: u32,
    /// A whole-pair capture at a tick strictly before `start` (from
    /// the player's keyframe store) to jump-start the re-sim at
    /// instead of simulating from boot: the pair boots unprimed and
    /// lands here, so a render carrying one pays for neither the
    /// priming walk nor the prefix. Without one the pair primes and
    /// the prefix simulates unwritten, same as a deselected round.
    ///
    /// A capture restore replaces the priming-time pokes, so callers
    /// wanting BGM muted must pass `None` and eat the full re-sim.
    pub snapshot: Option<Arc<tango_match::Capture>>,
    /// Inter-round transition ticks, as the recording's telemetry
    /// analysis found them (a recording holds no round marks of its
    /// own). The round ordinal at any tick — for `rounds_mask` indexing
    /// and chapter titles — is the count of marks at or before it; a
    /// jump-started pair couldn't answer that from live telemetry, so no
    /// render runs any. Empty is a legitimate answer, and means one
    /// chapter covering the clip: a caller with no analysis to hand
    /// renders the whole thing rather than guessing where to cut.
    pub round_marks: Vec<u32>,
}

impl std::fmt::Debug for Clip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Clip")
            .field("start", &self.start)
            .field("end", &self.end)
            .field("snapshot_tick", &self.snapshot.as_ref().map(|s| s.tick()))
            .field("round_marks", &self.round_marks)
            .finish()
    }
}

/// One replay render: which replay, which of it, and how it should
/// look. Everything a render needs that isn't an encoder, an output, or
/// a way to report back.
pub struct Request<'a> {
    /// The game's engine door — the recording's local seat, the same
    /// registration the player watches it through. The boot, the
    /// screen layout, and the frame clock all come from here, which is
    /// what keeps this crate ignorant of emulators.
    pub backend: &'a dyn tango_match::Backend,
    /// The recording's own header — the same boot the player uses, so
    /// the re-sim reproduces the recorded match. Its `local_player`
    /// names the side whose screen and audio track come first (the
    /// other one under [`Self::swap_sides`]).
    pub config: tango_match::ReplayConfig,
    /// Which rounds to write, indexed by [`Clip::round_marks`]
    /// interval. Unselected rounds still simulate; they just aren't
    /// written.
    pub rounds_mask: &'a [bool],
    /// Chapter title per round, indexed like `rounds_mask` (falls back
    /// to "Round N" past the end). Resolved by the host, which owns
    /// the locale.
    pub round_titles: &'a [String],
    /// The tick window to write.
    pub clip: &'a Clip,
    /// Render both seats — shown perspective first — with an audio
    /// track each, instead of the shown screen alone. A single-screen
    /// console's seats sit side by side; a multi-screen console (the
    /// DS) already fills its row with its own screens, so its seats
    /// stack vertically.
    pub twosided: bool,
    /// Show the opposite seat's perspective: the recording's remote
    /// side supplies the leading screen and audio track, the way the
    /// player presents the match while its swap toggle is on.
    /// Presentation only — the re-sim still boots from `config` as
    /// recorded.
    pub swap_sides: bool,
    /// The scale doubles as the quality choice, which is how the form
    /// presents it: one slider whose leftmost stop is raw output.
    /// `None` renders raw media at native size; `Some(n)` renders an
    /// `n`-times nearest-neighbor upscale.
    pub scale: Option<usize>,
}

/// Where a [`Render::pump`] left the render.
#[derive(Debug)]
pub enum Progress<T> {
    /// Still re-simulating: `done` of `total` ticks, counted from where
    /// the re-sim actually started (the snapshot tick for a
    /// jump-started clip) so a bar doesn't sit near-done from tick one.
    Rendering { done: usize, total: usize },
    /// Every frame is in and the media backend is finishing. Nothing
    /// left to feed; pump again to let it.
    Flushing,
    /// The container is closed and the writer is handed back.
    Done(T),
}

/// Which half of the render a driver is in.
enum Phase {
    Rendering,
    Flushing,
    Finished,
}

/// One audio track's path from emulator PCM to encoder PCM. Raw output
/// uses [`Passthrough`]; encoded output uses [`Resampler`].
/// The render loop doesn't need to know which one it owns.
trait AudioConverter: Send {
    /// Convert interleaved stereo `samples`, using `out` when conversion
    /// needs storage, and return the PCM to hand to the encoder.
    fn convert<'a>(&mut self, source_rate: f64, samples: &'a [i16], out: &'a mut Vec<i16>) -> &'a [i16];

    /// Forget any conversion state when a new written span begins.
    fn reset(&mut self) {}
}

/// Native-rate PCM handed straight through to the encoder.
struct Passthrough;

impl AudioConverter for Passthrough {
    fn convert<'a>(&mut self, _source_rate: f64, samples: &'a [i16], _out: &'a mut Vec<i16>) -> &'a [i16] {
        samples
    }
}

/// One audio track's offline rate conversion: samples staged at the
/// console's own rate, read out at the output's. The same linear
/// interpolation the live session plays through, minus the servo — an
/// export isn't paced, so the ratio is fixed and the whole stage
/// converts every take.
#[derive(Default)]
struct Resampler {
    /// Interleaved stereo at the console's rate.
    source: Vec<i16>,
    /// Fractional read position into `source`, in frames.
    cursor: f64,
}

impl Resampler {
    fn feed(&mut self, samples: &[i16]) {
        self.source.extend_from_slice(samples);
    }

    /// Convert everything staged into `out` at `step` source frames per
    /// output frame, keeping the tail frame the cursor still
    /// interpolates from. Returns output frames written.
    fn take(&mut self, step: f64, out: &mut Vec<i16>) -> usize {
        out.clear();
        if !step.is_finite() || step <= 0.0 {
            return 0;
        }
        let available = self.source.len() / 2;
        let mut written = 0;
        loop {
            let i = self.cursor as usize;
            if i + 1 >= available {
                break;
            }
            let frac = self.cursor - i as f64;
            for channel in 0..2 {
                let a = self.source[i * 2 + channel] as f64;
                let b = self.source[(i + 1) * 2 + channel] as f64;
                out.push((a + (b - a) * frac) as i16);
            }
            written += 1;
            self.cursor += step;
        }
        let consumed = (self.cursor as usize).min(available);
        if consumed > 0 {
            self.source.drain(..consumed * 2);
            self.cursor -= consumed as f64;
        }
        written
    }
}

impl AudioConverter for Resampler {
    fn convert<'a>(&mut self, source_rate: f64, samples: &'a [i16], out: &'a mut Vec<i16>) -> &'a [i16] {
        self.feed(samples);
        let frames = self.take(source_rate / LOSSY_SAMPLE_RATE, out);
        &out[..frames * AUDIO_CHANNELS]
    }

    fn reset(&mut self) {
        self.source.clear();
        self.cursor = 0.0;
    }
}

fn audio_converter(raw_output: bool) -> Box<dyn AudioConverter> {
    if raw_output {
        Box::new(Passthrough)
    } else {
        Box::new(Resampler::default())
    }
}

/// A running render of a recorded match.
///
/// One linear pair re-sim produces both perspectives at once, so the
/// two-sided layout is a compose of the two seats' frames rather than a
/// second simulation. A tick reaches the encoders when it's inside the
/// clip's span AND its round is selected in [`Request::rounds_mask`] —
/// the same ordering as a render form's round checkboxes, which come
/// from the same file marks. Unwritten spans still simulate; they just
/// aren't written. Each written round becomes a chapter in the output
/// container.
pub struct Render<W: Writer> {
    playback: tango_match::Playback,
    session: encoder_facade::Session,
    /// The container bytes' destination, wrapped in the appender +
    /// fixup applier every render needs from it. Taken at the close,
    /// which consumes it.
    output: Option<encoder_facade::Output<W>>,
    canceller: Canceller,
    phase: Phase,

    /// The seat whose screen and audio lead the output — the
    /// recording's local side, or the other one under
    /// [`Request::swap_sides`].
    first_seat: usize,
    twosided: bool,
    clip_start: u32,
    clip_end: u32,
    round_marks: Vec<u32>,
    rounds_mask: Vec<bool>,
    round_titles: Vec<String>,
    /// Last round the mask selects; past it there is nothing left to
    /// write, so the re-sim stops rather than simulating the tail.
    last_selected: Option<usize>,

    /// One seat's canvas — its screens stacked vertically (a single
    /// screen is the one-screen case) — and where each seat's lands in
    /// the output frame. Seats compose side by side.
    side_size: (u32, u32),
    /// The console's screens, for restacking each seat's canonical
    /// (side-by-side) frame into the vertical arrangement above.
    screens: Vec<tango_match::Screen>,
    /// The composed output frame, at its native (unscaled) size.
    frame: Vec<u8>,

    scratch: Vec<i16>,
    samples: Vec<i16>,
    audio_converters: [Box<dyn AudioConverter>; 2],
    prev_should_write: bool,

    /// Chapter bookkeeping, in output frames: the open chapter's
    /// (round, first written frame), closed when the written round
    /// changes or a mask gap stops writing.
    frames_written: u64,
    chapters: Vec<Chapter>,
    open_chapter: Option<(usize, u64)>,

    progress_base: usize,
    progress_total: usize,
}

impl<W: Writer> Render<W> {
    /// Boot the re-simulation and open the media backend.
    ///
    /// `open_output` opens the destination. The pair boots first so its
    /// audio rate can configure the raw stream; the output opens
    /// after the backend, so neither kind of failure can truncate a file
    /// it will never fill.
    pub fn new(
        request: Request<'_>,
        open_output: impl FnOnce() -> encoder_facade::Result<W>,
        canceller: &Canceller,
    ) -> Result<Self> {
        let Request {
            backend,
            config,
            rounds_mask,
            round_titles,
            clip,
            scale,
            twosided,
            swap_sides,
        } = request;
        canceller.check()?;
        let local_player = config.local_player;
        if local_player >= 2 {
            return Err(Error::BadLocalPlayer(local_player));
        }
        let first_seat = if swap_sides { 1 - local_player } else { local_player };

        // One seat's canvas is its screens stacked vertically — for a
        // single-screen console that's the screen itself — so each side
        // reads as one console; the output frame is one canvas, or the
        // two seats side by side.
        // A recording is a link battle, so the canvas is whatever that
        // mode composes — an export never carries a screen the match
        // itself didn't show.
        let layout = backend.screen_layout(tango_match::SessionMode::PvP {
            match_type: config.match_type,
        });
        let side_size = (
            layout.screens.iter().map(|s| s.width).max().unwrap_or(0),
            layout.screens.iter().map(|s| s.height).sum::<u32>(),
        );
        let (width, height) = if twosided {
            (side_size.0 * 2, side_size.1)
        } else {
            side_size
        };

        let raw_output = scale.is_none();
        let audio_tracks = if twosided { 2 } else { 1 };

        // Jump-start a clip from its capture: the pair starts at the
        // capture tick and only the (≤ one keyframe interval) gap to the
        // span start simulates unwritten. A capture at or after the span
        // start is no use — the clip's first frame has to come from a
        // stepped tick.
        let land_on = clip.snapshot.as_deref().filter(|c| c.tick() < clip.start);

        // Boot. Priming is encoder-free but bounded (~a few hundred
        // ticks), and it's the one part of a render that can't be
        // sliced: the pair primes by running until its traps say it's
        // there. A jump-started clip skips the walk outright — the
        // restore replaces everything it would have reached.
        let mut playback = backend.open_replay(config)?.linear(land_on)?;
        // Drop the audio the boot piled up (nothing drained during it).
        playback.discard_audio();

        // A raw render copies the emulator's PCM into Matroska and
        // describes its hardware clock exactly. Encoded AAC stays at
        // 48 kHz, with the offline converter below bridging any source
        // rate.
        let audio_sample_rate = if raw_output {
            let rate = playback.side(first_seat).audio_sample_rate();
            encoder_facade::SampleRate::new(rate.numerator, rate.denominator)
        } else {
            encoder_facade::SampleRate::integer(LOSSY_SAMPLE_RATE as u32)
        };
        let session = encoder_facade::Session::new(
            encoder_settings(
                scale,
                width,
                height,
                backend.frame_timing(),
                audio_sample_rate,
                audio_tracks,
            ),
            canceller,
        )?;
        let output = encoder_facade::Output::new(open_output()?);

        let progress_base = playback.cursor() as usize;
        // The re-sim stops at the last selected section's end (see
        // `render_slice`), so that end is the bar's 100% — an export of
        // only early sections (say, a random-battle recording's setup)
        // would otherwise finish with the bar barely started. The last
        // section has no closing mark; its selection means the re-sim
        // runs the clip out.
        let progress_stop = rounds_mask
            .iter()
            .rposition(|&s| s)
            .and_then(|last| clip.round_marks.get(last))
            .map_or(usize::MAX, |&t| t as usize);
        let progress_total = (clip.end as usize)
            .min(playback.total() as usize)
            .min(progress_stop)
            .saturating_sub(progress_base);
        Ok(Self {
            session,
            output: Some(output),
            canceller: canceller.clone(),
            phase: Phase::Rendering,
            first_seat,
            twosided,
            clip_start: clip.start,
            clip_end: clip.end,
            round_marks: clip.round_marks.clone(),
            rounds_mask: rounds_mask.to_vec(),
            round_titles: round_titles.to_vec(),
            last_selected: rounds_mask.iter().rposition(|&s| s),
            side_size,
            screens: layout.screens.clone(),
            frame: vec![0u8; (width * height * 4) as usize],
            scratch: vec![0i16; 16384 * AUDIO_CHANNELS],
            samples: Vec::new(),
            audio_converters: std::array::from_fn(|_| audio_converter(raw_output)),
            prev_should_write: false,
            frames_written: 0,
            chapters: vec![],
            open_chapter: None,
            progress_base,
            progress_total,
            playback,
        })
    }

    /// Advance the render by at most `max_ticks` re-simulated ticks, or
    /// drive one step of the close, and report where that left it.
    ///
    /// A slice also ends early when the encoders are running behind
    /// ([`MAX_ENCODER_QUEUE`]), so a host that can't block still can't
    /// outrun them. Pump again after giving the encoders a turn — on an
    /// event loop that means yielding to it, which is where a browser's
    /// encoders do their work.
    pub fn pump(&mut self, max_ticks: usize) -> Result<Progress<W>> {
        match self.phase {
            Phase::Rendering => {
                if self.render_slice(max_ticks)? {
                    return Ok(Progress::Rendering {
                        done: (self.playback.cursor() as usize).saturating_sub(self.progress_base),
                        total: self.progress_total,
                    });
                }
                if let Some(open) = self.open_chapter.take() {
                    self.close_chapter(open);
                }
                self.session.begin_finish()?;
                self.phase = Phase::Flushing;
                Ok(Progress::Flushing)
            }
            Phase::Flushing => {
                self.canceller.check()?;
                let Some(fixups) = self.session.poll_finish(&self.chapters)? else {
                    return Ok(Progress::Flushing);
                };
                let mut output = self.output.take().ok_or(Error::AlreadyFinished)?;
                output.append(&self.session.take_output())?;
                let written = output.finish(&fixups)?;
                self.phase = Phase::Finished;
                Ok(Progress::Done(written))
            }
            Phase::Finished => Err(Error::AlreadyFinished),
        }
    }

    /// Re-simulate up to `max_ticks` ticks, writing the ones that
    /// belong to the render. `true` if there are more to come.
    fn render_slice(&mut self, max_ticks: usize) -> Result<bool> {
        for _ in 0..max_ticks {
            if self.session.queue_depth() > MAX_ENCODER_QUEUE {
                return Ok(true);
            }
            self.canceller.check()?;
            if !self.playback.step() {
                return Ok(false);
            }
            let tick = self.playback.cursor();
            let cur_round = self.round_marks.partition_point(|&m| m <= tick);
            if tick > self.clip_end || self.last_selected.is_none_or(|last| cur_round > last) {
                return Ok(false);
            }
            self.write_tick(tick, cur_round)?;
        }
        Ok(true)
    }

    /// One re-simulated tick: gate it, blit and drain what it produced,
    /// and pass the encoders' output on to the sink.
    fn write_tick(&mut self, tick: u32, cur_round: usize) -> Result<()> {
        let should_write = tick >= self.clip_start && self.rounds_mask.get(cur_round).copied().unwrap_or(false);
        if let Some(open) = self.open_chapter {
            if !should_write || open.0 != cur_round {
                self.close_chapter(open);
                self.open_chapter = None;
            }
        }
        if should_write && self.open_chapter.is_none() {
            self.open_chapter = Some((cur_round, self.frames_written));
        }
        if should_write && !self.prev_should_write {
            for converter in &mut self.audio_converters {
                converter.reset();
            }
        }
        self.prev_should_write = should_write;

        // Drain each seat's tick of audio; raw PCM goes straight
        // through at the emulator's rate, while encoded audio is
        // converted to 48 kHz. Then blit the seat's frame.
        // Track/screen order: shown perspective first. An unwritten
        // tick still drains — the consoles' own rings are small, and
        // sound left there would open the next span as a stale burst —
        // it just goes nowhere.
        let order: [usize; 2] = [self.first_seat, 1 - self.first_seat];
        for (slot, &seat) in order.iter().enumerate() {
            if !self.twosided && slot > 0 {
                break;
            }
            let mut side = self.playback.side(seat);
            let rate = side.audio_sample_rate().as_f64();
            loop {
                // What landed is whatever fit: a drain fills as far as
                // it goes and reports the console's whole total.
                let written = side
                    .drain_audio(&mut self.scratch)
                    .min(self.scratch.len() / AUDIO_CHANNELS);
                if written == 0 {
                    break;
                }
                if should_write {
                    let drained = &self.scratch[..written * AUDIO_CHANNELS];
                    let samples = self.audio_converters[slot].convert(rate, drained, &mut self.samples);
                    self.session.write_audio(slot, samples)?;
                }
            }
            if should_write {
                if let Some(fb) = side.frame() {
                    let width = if self.twosided {
                        self.side_size.0 as usize * 2
                    } else {
                        self.side_size.0 as usize
                    };
                    blit_seat(
                        &mut self.frame,
                        width,
                        slot * self.side_size.0 as usize,
                        &self.screens,
                        &fb,
                    );
                }
            }
        }
        if should_write {
            self.session.write_video(&self.frame)?;
            self.frames_written += 1;
        }
        // Whatever the encoders have finished goes to the output as
        // the render runs, so memory stays flat however long the
        // replay is.
        let bytes = self.session.take_output();
        self.output.as_mut().ok_or(Error::AlreadyFinished)?.append(&bytes)?;
        Ok(())
    }

    fn close_chapter(&mut self, (round, start): (usize, u64)) {
        if self.frames_written > start {
            let title = self
                .round_titles
                .get(round)
                .cloned()
                .unwrap_or_else(|| format!("Round {}", round + 1));
            self.chapters.push(Chapter {
                title,
                start_frame: start,
                end_frame: self.frames_written,
            });
        }
    }
}

/// Run a render to completion on the calling thread, reporting
/// `(done, total)` ticks as it goes — the shape a host with a thread to
/// spare wants. A browser host drives [`Render`] from its event loop
/// instead; see the module docs.
pub fn render<W: Writer>(
    request: Request<'_>,
    open_output: impl FnOnce() -> encoder_facade::Result<W>,
    canceller: &Canceller,
    progress_callback: impl Fn(usize, usize),
) -> Result<W> {
    let mut render = Render::new(request, open_output, canceller)?;
    loop {
        match render.pump(BLOCKING_SLICE_TICKS)? {
            Progress::Rendering { done, total } => progress_callback(done, total),
            Progress::Flushing => {}
            Progress::Done(done) => return Ok(done),
        }
    }
}

/// Copy one seat's frame onto the output frame in its column at `x`,
/// restacking the screens vertically. The seam's frame buffer is the
/// canonical composition — one row-major RGBA8 bitmap, the console's
/// screens left to right ([`tango_match::Side::frame`]) — and the
/// output lays those same screens top to bottom instead; a
/// single-screen console is the one-screen case of the same walk. A
/// short buffer (a seat that hasn't drawn yet) blits the rows it has
/// and leaves the rest.
fn blit_seat(dst: &mut [u8], dst_width: usize, x: usize, screens: &[tango_match::Screen], src: &[u8]) {
    let src_stride = screens.iter().map(|s| s.width as usize).sum::<usize>() * 4;
    let mut src_x = 0usize;
    let mut y = 0usize;
    for screen in screens {
        let stride = screen.width as usize * 4;
        for row in 0..screen.height as usize {
            let from = row * src_stride + src_x * 4;
            let Some(line) = src.get(from..from + stride) else {
                continue;
            };
            let at = ((y + row) * dst_width + x) * 4;
            dst[at..at + stride].copy_from_slice(line);
        }
        src_x += screen.width as usize;
        y += screen.height as usize;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every pixel says which seat and row it came from, so a blit that
    /// slipped by a row or a slot still fills the frame but fails the
    /// per-pixel check. Row-major, like the seam's composed frame.
    fn seat_frame(tag: u8, w: usize, h: usize) -> Vec<u8> {
        (0..w * h).flat_map(|i| [tag, (i / w) as u8, (i % w) as u8, 0xff]).collect()
    }

    /// A single-screen console's two seats land side by side, row for
    /// row.
    #[test]
    fn single_screen_seats_compose_side_by_side() {
        let (w, h) = (240usize, 160usize);
        let screens = [tango_match::Screen {
            width: w as u32,
            height: h as u32,
        }];
        let (left, right) = (seat_frame(1, w, h), seat_frame(2, w, h));
        let mut composed = vec![0u8; w * 2 * h * 4];
        blit_seat(&mut composed, w * 2, 0, &screens, &left);
        blit_seat(&mut composed, w * 2, w, &screens, &right);

        for row in 0..h {
            let line = &composed[row * w * 2 * 4..(row + 1) * w * 2 * 4];
            assert_eq!(&line[..w * 4], &left[row * w * 4..(row + 1) * w * 4], "row {row} left");
            assert_eq!(&line[w * 4..], &right[row * w * 4..(row + 1) * w * 4], "row {row} right");
        }
    }

    /// A multi-screen console's seat restacks its screens vertically:
    /// the seam hands one row-major bitmap with the screens left to
    /// right ([`tango_match::Side::frame`]), and the seat's column in
    /// the output carries screen 0's rows, then screen 1's.
    #[test]
    fn multi_screen_seat_restacks_screens_vertically() {
        // A DS-shaped seat: two 8-wide screens composed into a 16-wide
        // row-major frame by the engine.
        let (sw, sh) = (8usize, 4usize);
        let screens = [
            tango_match::Screen {
                width: sw as u32,
                height: sh as u32,
            },
            tango_match::Screen {
                width: sw as u32,
                height: sh as u32,
            },
        ];
        let canonical = seat_frame(1, sw * 2, sh);
        let mut composed = vec![0u8; sw * sh * 2 * 4];
        blit_seat(&mut composed, sw, 0, &screens, &canonical);

        let canonical_line = |y: usize, x: usize| &canonical[(y * sw * 2 + x) * 4..(y * sw * 2 + x + sw) * 4];
        let line = |y: usize| &composed[y * sw * 4..(y + 1) * sw * 4];
        for row in 0..sh {
            assert_eq!(line(row), canonical_line(row, 0), "upper screen row {row}");
            assert_eq!(line(row + sh), canonical_line(row, sw), "lower screen row {row}");
        }
    }

    /// Two multi-screen seats land side by side, each restacked into
    /// its own column.
    #[test]
    fn multi_screen_seats_compose_side_by_side() {
        let (sw, sh) = (8usize, 4usize);
        let screens = [
            tango_match::Screen {
                width: sw as u32,
                height: sh as u32,
            },
            tango_match::Screen {
                width: sw as u32,
                height: sh as u32,
            },
        ];
        let (left, right) = (seat_frame(1, sw * 2, sh), seat_frame(2, sw * 2, sh));
        let out_w = sw * 2;
        let mut composed = vec![0u8; out_w * sh * 2 * 4];
        blit_seat(&mut composed, out_w, 0, &screens, &left);
        blit_seat(&mut composed, out_w, sw, &screens, &right);

        for row in 0..sh * 2 {
            let line = &composed[row * out_w * 4..(row + 1) * out_w * 4];
            // Each column is its seat's tag throughout.
            assert!(line[..sw * 4].chunks_exact(4).all(|px| px[0] == 1), "row {row} left");
            assert!(line[sw * 4..].chunks_exact(4).all(|px| px[0] == 2), "row {row} right");
        }
    }

    /// A seat that hasn't drawn yet hands back an empty buffer; the
    /// blit leaves the frame alone rather than panicking or smearing.
    #[test]
    fn an_undrawn_seat_blits_nothing() {
        let mut composed = vec![7u8; 240 * 160 * 4];
        let screens = [tango_match::Screen { width: 240, height: 160 }];
        blit_seat(&mut composed, 240, 0, &screens, &[]);
        assert!(composed.iter().all(|&b| b == 7));
    }

    /// The offline resampler holds the ratio: 32768 Hz in, 48 kHz out,
    /// and the output length tracks input × ratio with only the
    /// interpolation tail withheld.
    #[test]
    fn the_resampler_holds_its_ratio() {
        let mut resampler = Resampler::default();
        let mut out = Vec::new();
        let mut total = 0usize;
        for _ in 0..100 {
            let input = vec![100i16; 548 * 2];
            total += resampler.convert(32_768.0, &input, &mut out).len() / AUDIO_CHANNELS;
        }
        let want = (548.0 * 100.0 * 48000.0 / 32768.0) as usize;
        assert!(total.abs_diff(want) < 4, "{total} output frames for {want} expected");
    }

    #[test]
    fn passthrough_returns_native_pcm_directly() {
        let input = vec![-32_768, 32_767, -123, 456];
        let mut out = vec![7; input.len()];
        let samples = Passthrough.convert(32_768.0, &input, &mut out);
        assert_eq!(samples, input);
        assert_eq!(samples.as_ptr(), input.as_ptr());
    }

    #[test]
    fn native_sample_rates_preserve_the_hardware_clocks() {
        let gba = tango_match::AudioSampleRate::integer(32_768);
        assert_eq!(gba.as_f64(), 32_768.0);
        let ds = tango_match::AudioSampleRate::new(33_513_982, 1_024);
        assert_eq!(ds.as_f64(), 33_513_982.0 / 1_024.0);
        let encoded_ds = encoder_facade::SampleRate::new(ds.numerator, ds.denominator);

        let settings = encoder_settings(
            None,
            240,
            160,
            tango_match::FrameTiming {
                timescale: 16_777_216,
                frame_duration: 280_896,
            },
            encoded_ds,
            1,
        );
        assert_eq!(settings.video.codec, encoder_facade::VideoCodec::RawRgb24);
        assert_eq!(settings.audio.codec, encoder_facade::AudioCodec::PcmS16Le);
        assert_eq!(settings.audio.sample_rate, encoded_ds);
        assert_eq!(settings.container, encoder_facade::Container::Matroska);
    }
}
