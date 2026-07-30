//! Turning a recorded replay back into video: re-simulates it through
//! the seam's replay path ([`tango_match::ReplaySet`]) and feeds the
//! frames and audio to [`encoder_facade`], which encodes and muxes them
//! into a video file.
//!
//! Engine-neutral on purpose: the boot comes from the game's own
//! [`tango_match::Backend`] — the same registration the player watches
//! replays through — and everything read back (RGBA8 frames, interleaved
//! audio, the console's screen layout and frame clock) is the seam's,
//! so a GBA replay and a DS replay render through the same pipeline.
//!
//! Nothing here picks an encoder or opens a file. [`encoder_facade`]
//! starts whichever encoder the target has (ffmpeg subprocesses
//! natively, WebCodecs in a browser), and the host passes in something
//! seekable to write to — a `File`, a `Cursor<Vec<u8>>`, a shim over an
//! OPFS sync handle — so the same re-simulation serves both.
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
/// dialog should offer. Lossless is RGB H.264 with FLAC, neither of
/// which MP4 carries, so it goes to Matroska.
pub fn container(lossless: bool) -> encoder_facade::Container {
    if lossless {
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
    audio_tracks: usize,
) -> encoder_facade::Settings {
    let lossless = scale.is_none();
    encoder_facade::Settings {
        video: encoder_facade::VideoSettings {
            // A lossless render stays in RGB; a lossy one is CRF-rated,
            // which at this frame size is predictable where a bitrate
            // target would swing with the content.
            codec: encoder_facade::VideoCodec::H264 {
                quality: if lossless {
                    encoder_facade::H264Quality::Lossless
                } else {
                    encoder_facade::H264Quality::Crf(18)
                },
            },
            width,
            height,
            scale: scale.unwrap_or(1) as u32,
            keyframe_interval: KEYFRAME_INTERVAL,
            timescale: timing.timescale,
            frame_duration: timing.frame_duration,
            // An RGB H.264 stream can't carry these at all.
            color: (!lossless).then_some(encoder_facade::ColorInfo::SRGB_FULL),
        },
        audio: encoder_facade::AudioSettings {
            codec: if lossless {
                encoder_facade::AudioCodec::Flac
            } else {
                encoder_facade::AudioCodec::Aac { bitrate: 384_000 }
            },
            sample_rate: SAMPLE_RATE as u32,
            channels: AUDIO_CHANNELS as u8,
        },
        container: container(lossless),
        audio_tracks,
        // What the old pipeline's `-movflags +faststart` did: the
        // index goes in front of the media, so a player reading the
        // file in order can start before it has all of it. Matroska
        // ignores it.
        faststart: !lossless,
    }
}

const SAMPLE_RATE: f64 = 48000.0;

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
    /// instead of simulating from boot. Without one the prefix
    /// simulates unwritten, same as a deselected round.
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
    /// names the side whose screen and audio track come first.
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
    /// The scale doubles as the quality choice, which is how the form
    /// presents it: one slider whose leftmost stop is lossless.
    /// `None` renders losslessly at native size; `Some(n)` renders an
    /// `n`-times nearest-neighbor upscale.
    pub twosided: bool,
    /// Render both seats — local perspective first — with an audio
    /// track each, instead of the local screen alone. A single-screen
    /// console's seats sit side by side; a multi-screen console (the
    /// DS) already fills its row with its own screens, so its seats
    /// stack vertically.
    pub scale: Option<usize>,
}

/// Where a [`Render::pump`] left the render.
#[derive(Debug)]
pub enum Progress<T> {
    /// Still re-simulating: `done` of `total` ticks, counted from where
    /// the re-sim actually started (the snapshot tick for a
    /// jump-started clip) so a bar doesn't sit near-done from tick one.
    Rendering { done: usize, total: usize },
    /// Every frame is in and the encoders are finishing. Nothing left
    /// to feed; pump again to let them.
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

    /// Forget everything staged — for a write gap's restart, so a new
    /// span doesn't open on the tail of a span the viewer never saw.
    fn reset(&mut self) {
        self.source.clear();
        self.cursor = 0.0;
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

    local_player: usize,
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
    resamplers: [Resampler; 2],
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
    /// Boot the re-simulation and open the encoders.
    ///
    /// `open_output` opens the destination. The encoders start first, so
    /// a missing one fails before seconds of CPU go into the re-sim, and
    /// the output opens last, so a broken encoder can't truncate a file
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
        } = request;
        canceller.check()?;
        let local_player = config.local_player;
        if local_player >= 2 {
            return Err(Error::BadLocalPlayer(local_player));
        }

        // One seat's canvas is its screens stacked vertically — for a
        // single-screen console that's the screen itself — so each side
        // reads as one console; the output frame is one canvas, or the
        // two seats side by side.
        let layout = backend.screen_layout();
        let side_size = (
            layout.screens.iter().map(|s| s.width).max().unwrap_or(0),
            layout.screens.iter().map(|s| s.height).sum::<u32>(),
        );
        let (width, height) = if twosided {
            (side_size.0 * 2, side_size.1)
        } else {
            side_size
        };

        let audio_tracks = if twosided { 2 } else { 1 };
        let session = encoder_facade::Session::new(
            encoder_settings(scale, width, height, backend.frame_timing(), audio_tracks),
            canceller,
        )?;
        let output = encoder_facade::Output::new(open_output()?);

        // Boot + prime. This is encoder-free but bounded (~a few hundred
        // ticks), and it's the one part of a render that can't be
        // sliced: the pair primes by running until its traps say it's
        // there.
        let mut playback = backend.open_replay(config)?.linear()?;
        // Drop the audio priming piled up (nothing drained during boot).
        playback.discard_audio();

        // Jump-start a clip from its capture: the pair skips straight to
        // the capture tick and only the (≤ one keyframe interval) gap to
        // the span start simulates unwritten.
        if let Some(capture) = clip.snapshot.as_deref() {
            if capture.tick() < clip.start {
                playback.load(capture)?;
                playback.discard_audio();
            }
        }

        let progress_base = playback.cursor() as usize;
        let progress_total = (clip.end as usize)
            .min(playback.total() as usize)
            .saturating_sub(progress_base);
        Ok(Self {
            session,
            output: Some(output),
            canceller: canceller.clone(),
            phase: Phase::Rendering,
            local_player,
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
            resamplers: [Resampler::default(), Resampler::default()],
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
            for r in &mut self.resamplers {
                r.reset();
            }
        }
        self.prev_should_write = should_write;

        // Drain + resample each seat's tick of audio; blit its frame.
        // Track/screen order: local perspective first. An unwritten
        // tick still drains — the consoles' own rings are small, and
        // sound left there would open the next span as a stale burst —
        // it just goes nowhere.
        let order: [usize; 2] = [self.local_player, 1 - self.local_player];
        for (slot, &seat) in order.iter().enumerate() {
            if !self.twosided && slot > 0 {
                break;
            }
            let mut side = self.playback.side(seat);
            let rate = side.audio_sample_rate();
            loop {
                let drained = side.drain_audio(&mut self.scratch);
                self.resamplers[slot].feed(&self.scratch[..drained.written * AUDIO_CHANNELS]);
                if drained.written == 0 {
                    break;
                }
            }
            let n = self.resamplers[slot].take(rate / SAMPLE_RATE, &mut self.samples);
            if should_write {
                self.session.write_audio(slot, &self.samples[..n * AUDIO_CHANNELS])?;
                if let Some(fb) = side.frame() {
                    let width = if self.twosided {
                        self.side_size.0 as usize * 2
                    } else {
                        self.side_size.0 as usize
                    };
                    blit_seat(&mut self.frame, width, slot * self.side_size.0 as usize, &self.screens, &fb);
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
            resampler.feed(&vec![100i16; 548 * 2]);
            total += resampler.take(32768.0 / 48000.0, &mut out);
        }
        let want = (548.0 * 100.0 * 48000.0 / 32768.0) as usize;
        assert!(total.abs_diff(want) < 4, "{total} output frames for {want} expected");
    }
}
