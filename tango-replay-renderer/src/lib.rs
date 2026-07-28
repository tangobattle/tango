//! Turning a recorded replay back into video: re-simulates it through
//! [`tango_backend_mgba::r#match::playback`] and feeds the frames and audio to
//! [`encoder_facade`], which encodes and muxes them into a video file.
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
    Engine(#[from] tango_backend_mgba::Error),
    /// A replay naming a side the two-core pair doesn't have.
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

/// Translate a render's scale choice into encoder settings.
/// `width_screens` is the output width in GBA screens (1 one-sided, 2
/// side-by-side), with one audio track per screen.
fn encoder_settings(scale: Option<usize>, width_screens: u32, audio_tracks: usize) -> encoder_facade::Settings {
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
            width: mgba::gba::SCREEN_WIDTH * width_screens,
            height: mgba::gba::SCREEN_HEIGHT,
            scale: scale.unwrap_or(1) as u32,
            keyframe_interval: KEYFRAME_INTERVAL,
            timescale: TIMESCALE,
            frame_duration: FRAME_DURATION,
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

/// The GBA frame clock: 280896 cycles at 2^24 Hz, stated as a timebase
/// so the encoder can time frames exactly rather than in rounded
/// milliseconds.
const TIMESCALE: u32 = 16_777_216;
const FRAME_DURATION: u64 = 280_896;

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
    /// A whole-pair savestate at a tick strictly before `start` (from
    /// the player's keyframe store) to jump-start the re-sim at
    /// instead of simulating from boot. Without one the prefix
    /// simulates unwritten, same as a deselected round.
    ///
    /// A savestate restore replaces the priming-time pokes, so callers
    /// wanting BGM muted must pass `None` and eat the full re-sim.
    pub snapshot: Option<Arc<tango_backend_mgba::r#match::playback::Snapshot>>,
    /// Inter-round transition ticks ([`tango_replay::Replay`]'s
    /// `round_starts` minus the leading 0, or the player's discovered
    /// boundaries for recordings that predate the markers). The round
    /// ordinal at any tick — for `rounds_mask` indexing and chapter
    /// titles — is the count of marks at or before it; a jump-started
    /// pair couldn't answer that from live telemetry, so no render
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

/// One replay render: which replay, which of it, and how it should
/// look. Everything a render needs that isn't an encoder, an output, or
/// a way to report back.
pub struct Request<'a> {
    /// Both sides' ROMs, saves and match settings — the same boot the
    /// player uses, so the re-sim reproduces the recorded match.
    pub config: &'a tango_backend_mgba::r#match::playback::BootConfig,
    /// The recorded input stream, one pair per tick.
    pub inputs: &'a [[u32; 2]],
    /// Which side the replay was recorded from. Its screen and audio
    /// track come first.
    pub local_player: usize,
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
    pub scale: Option<usize>,
    /// Render both screens side by side, local perspective on the
    /// left, with an audio track each — instead of the local screen
    /// alone.
    pub twosided: bool,
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

/// A running render of an SIO replay ([`tango_replay::VERSION`]).
///
/// One linear pair re-sim produces both perspectives at once, so the
/// two-sided layout is a compose of the two framebuffers rather than a
/// second simulation. A tick reaches the encoders when it's inside the
/// clip's span AND its round is selected in [`Request::rounds_mask`] —
/// the same ordering as a render form's round checkboxes, which come
/// from the same file marks. Unwritten spans still simulate; they just
/// aren't written. Each written round becomes a chapter in the output
/// container.
pub struct Render<W: Writer> {
    playback: tango_backend_mgba::r#match::playback::Playback,
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

    vbuf: Vec<u8>,
    composed_vbuf: Vec<u8>,
    samples: Vec<i16>,
    resamplers: [mgba::audio::AudioResampler; 2],
    dest_buffers: [mgba::audio::OwnedAudioBuffer; 2],
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
        request: &Request<'_>,
        open_output: impl FnOnce() -> encoder_facade::Result<W>,
        canceller: &Canceller,
    ) -> Result<Self> {
        let &Request {
            config,
            inputs,
            local_player,
            rounds_mask,
            round_titles,
            clip,
            scale,
            twosided,
        } = request;
        canceller.check()?;
        if local_player >= 2 {
            return Err(Error::BadLocalPlayer(local_player));
        }

        let (width_screens, audio_tracks) = if twosided { (2, 2) } else { (1, 1) };
        let session = encoder_facade::Session::new(encoder_settings(scale, width_screens, audio_tracks), canceller)?;
        let output = encoder_facade::Output::new(open_output()?);

        // Boot + prime. This is encoder-free but bounded (~a few hundred
        // ticks), and it's the one part of a render that can't be
        // sliced: the pair primes by running until its traps say it's
        // there.
        let lifecycle = tango_backend_mgba::r#match::telemetry::LifecycleSink::new();
        let mut playback = tango_backend_mgba::r#match::playback::Playback::new(config, Arc::new(inputs.to_vec()), &lifecycle)?;
        // Drop the audio priming piled up (nothing drained during boot).
        for i in 0..2 {
            playback.pair_mut().core_mut(i).audio_buffer().clear();
        }

        // Jump-start a clip from its snapshot: the pair skips straight to
        // the capture tick and only the (≤ one keyframe interval) gap to
        // the span start simulates unwritten.
        if let Some(snap) = clip.snapshot.as_deref() {
            if snap.tick < clip.start {
                playback.load(snap)?;
                for i in 0..2 {
                    playback.pair_mut().core_mut(i).audio_buffer().clear();
                }
            }
        }

        let (w, h) = (mgba::gba::SCREEN_WIDTH, mgba::gba::SCREEN_HEIGHT);
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
            vbuf: vec![0u8; (w * h * 4) as usize],
            composed_vbuf: vec![0u8; (w * 2 * h * 4) as usize],
            samples: vec![0i16; SAMPLE_RATE as usize],
            resamplers: [mgba::audio::AudioResampler::new(), mgba::audio::AudioResampler::new()],
            dest_buffers: [
                mgba::audio::OwnedAudioBuffer::new(0x4000, AUDIO_CHANNELS as u32),
                mgba::audio::OwnedAudioBuffer::new(0x4000, AUDIO_CHANNELS as u32),
            ],
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
            for (r, d) in self.resamplers.iter_mut().zip(self.dest_buffers.iter_mut()) {
                *r = mgba::audio::AudioResampler::new();
                d.clear();
            }
        }
        self.prev_should_write = should_write;

        // Drain + resample each core's tick of audio; blit its frame.
        // Track/screen order: local perspective first.
        let order: [usize; 2] = [self.local_player, 1 - self.local_player];
        for (slot, &core_idx) in order.iter().enumerate() {
            if !self.twosided && slot > 0 {
                break;
            }
            let pair = self.playback.pair_mut();
            let n = {
                let core = pair.core_mut(core_idx);
                let core_rate = core.audio_sample_rate() as f64;
                let core_buffer = core.audio_buffer();
                self.resamplers[slot].set_source(core_buffer, core_rate, true);
                self.resamplers[slot].set_destination(&mut self.dest_buffers[slot], SAMPLE_RATE);
                self.resamplers[slot].process();
                let cap = self.samples.len() / AUDIO_CHANNELS;
                let frames = self.dest_buffers[slot].available().min(cap);
                self.dest_buffers[slot].read(&mut self.samples[..frames * AUDIO_CHANNELS], frames);
                frames
            };
            if should_write {
                self.session.write_audio(slot, &self.samples[..n * AUDIO_CHANNELS])?;
                if let Some(fb) = pair.video_buffer(core_idx) {
                    mgba::gba::bgr555_to_rgba8(fb, &mut self.vbuf);
                    if self.twosided {
                        blit_screen(&mut self.composed_vbuf, slot, &self.vbuf);
                    }
                }
            }
        }
        if should_write {
            let frame = if self.twosided { &self.composed_vbuf } else { &self.vbuf };
            self.session.write_video(frame)?;
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
    request: &Request<'_>,
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

/// Copy one screen's RGBA pixels into the `slot`-th half of a
/// side-by-side frame — both tightly packed, the destination two
/// screens wide.
fn blit_screen(dst: &mut [u8], slot: usize, src: &[u8]) {
    let stride = mgba::gba::SCREEN_WIDTH as usize * 4;
    for (row, line) in src.chunks_exact(stride).enumerate() {
        let at = row * stride * 2 + slot * stride;
        dst[at..at + stride].copy_from_slice(line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each screen lands in its own half, row for row — a compose that
    /// slipped by a row or a slot would still fill the frame, so the
    /// check is per-pixel rather than "something got written".
    #[test]
    fn a_composed_frame_keeps_each_screen_on_its_own_side() {
        let (w, h) = (mgba::gba::SCREEN_WIDTH as usize, mgba::gba::SCREEN_HEIGHT as usize);
        // Every pixel says which row it came from, so a shift shows up.
        let screen = |tag: u8| -> Vec<u8> {
            (0..w * h)
                .flat_map(|i| [tag, (i / w) as u8, (i % w) as u8, 0xff])
                .collect()
        };
        let (left, right) = (screen(1), screen(2));
        let mut composed = vec![0u8; w * 2 * h * 4];
        blit_screen(&mut composed, 0, &left);
        blit_screen(&mut composed, 1, &right);

        for row in 0..h {
            let line = &composed[row * w * 2 * 4..(row + 1) * w * 2 * 4];
            assert_eq!(&line[..w * 4], &left[row * w * 4..(row + 1) * w * 4], "row {row} left");
            assert_eq!(
                &line[w * 4..],
                &right[row * w * 4..(row + 1) * w * 4],
                "row {row} right"
            );
        }
    }
}

