//! Replay playback session: a recording re-simulated on the game's own
//! engine ([`tango_match::RunningReplay`]) behind a mutex, paced by a
//! drive thread, with a second pass racing ahead of the playhead for
//! the match statistics and the keyframes that make seeking cheap.
//!
//! Seeks are asynchronous: requests land on a
//! [`SeekController`](tango_match::seek::SeekController) and a
//! dedicated worker chases the newest target a slice at a time, so the
//! UI never blocks on catch-up emulation. What the engine keeps to
//! serve those chases — keyframes, a rewind ring — is the engine's
//! business; this session only says where to go and how much time it
//! may take getting there.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use tango_match::seek::SeekController;

/// A GBA screen, which is what every game with replays is on today.
pub const SCREEN_WIDTH: u32 = 240;
pub const SCREEN_HEIGHT: u32 = 160;
const EXPECTED_FPS: f32 = 60.0;

/// What the input display overlay reads off a replay: every recorded
/// (local, remote) joyflags pair, flattened across rounds in playhead
/// order (index = the tick that consumed it) and masked to the
/// hardware bits, plus the two sides' nicknames for the chip captions.
struct InputDisplay {
    pairs: Vec<(u16, u16)>,
    nicknames: (String, String),
}

pub struct ReplaySession {
    game: &'static tango_gamesupport::Game,
    /// Inter-round seek-bar marks (see [`Self::round_boundaries`]),
    /// discovered from telemetry by the prefetch pass as it runs.
    round_boundaries: Arc<Mutex<Vec<u32>>>,
    total_ticks: u32,
    /// Input display lookup data ([`Self::input_at`] /
    /// [`Self::nicknames`]). Boxed to keep this struct — and with it
    /// the `Session` enum — small, same as the PvP variant.
    input_display: Box<InputDisplay>,
    /// The console's screens, as the local game's engine presents them
    /// — what both surfaces below are sized for.
    layout: tango_match::ScreenLayout,
    /// This session's display, kept so [`Self::scrub_preview`] can blit
    /// snapshot framebuffers without going through the emulator at all.
    screen: Arc<crate::Framebuffer>,
    /// Repaint wake, fired once per published frame (and per blit).
    wake: Arc<tokio::sync::Notify>,
    /// Whether the opponent-screen PiP is on (a per-session toggle on
    /// the transport bar).
    show_pip: Arc<AtomicBool>,
    /// Whether the main screen shows the opponent's perspective instead
    /// of the local one — a per-session toggle on the transport bar. The
    /// PiP, when also on, carries the local screen so the two surfaces
    /// always show both sides.
    swap_perspective: Arc<AtomicBool>,
    /// The opponent's screen, written once per published frame while
    /// the PiP is on.
    pip: Arc<crate::Framebuffer>,
    /// Whether `pip` holds a frame from the current PiP activation
    /// (cleared while off, so a stale capture never flashes on re-toggle).
    pip_fresh: Arc<AtomicBool>,
    /// The playback machinery (pair, workers, seek state).
    engine: Engine,
}

/// Playback: the recording's pair behind a mutex, paced by a host
/// drive thread; the seek worker chases targets through the engine's
/// own captures, and the prefetch worker races a second simulation
/// ahead for keyframes, statistics and round marks.
struct Engine {
    /// Which pair core is the replay's local perspective.
    local_player: usize,
    /// Lock-free playhead mirror for UI reads.
    cursor: Arc<AtomicU32>,
    paused: Arc<crate::PauseGate>,
    /// Pacing target, f32 bits (60 × speed factor).
    fps_bits: Arc<AtomicU32>,
    /// The recording as the game's engine offers it: the pair below and
    /// the statistics pass share the captures they lay down.
    set: Arc<dyn tango_match::ReplaySet>,
    playback: SharedPlayback,
    prefetch_progress: Arc<AtomicU32>,
    seek: Arc<SeekController>,
    /// Cancels the loops the host is running, on Drop; whoever is
    /// running them notices on their next tick (and a host with threads
    /// joins them afterwards).
    cancel: Arc<AtomicBool>,
}

type SharedPlayback = Arc<Mutex<Option<Box<dyn tango_match::RunningReplay>>>>;

impl Drop for Engine {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        // The engine's own flag, checked per tick deep inside the stats
        // pass and both pairs' priming walks — without it a host joining
        // its workers waits out whatever slice or boot is in flight.
        self.set.cancel();
        // Release a gate-parked drive loop so the host's join is prompt.
        self.paused.set(false);
        self.seek.shutdown();
    }
}

/// The session's audio before its pair exists.
///
/// A host binds its output stream when the session is built, but
/// playback boots on the drive loop's first tick — a second or two of
/// priming later. This stands in until then, reporting silence, and
/// starts delegating the moment the engine hands over a real pull.
#[derive(Clone, Default)]
struct DeferredPull(Arc<Mutex<Option<Box<dyn tango_match::AudioPull>>>>);

impl DeferredPull {
    fn set(&self, pull: Box<dyn tango_match::AudioPull>) {
        *self.0.lock().unwrap() = Some(pull);
    }

    fn with<R>(&self, fallback: R, f: impl FnOnce(&mut Box<dyn tango_match::AudioPull>) -> R) -> R {
        match self.0.lock().unwrap().as_mut() {
            Some(pull) => f(pull),
            None => fallback,
        }
    }
}

impl tango_match::AudioPull for DeferredPull {
    fn sample_rate(&self) -> f64 {
        // The GBA's own rate, which is what every replayable game
        // produces at and only stands in while the pair is booting.
        self.with(32768.0, |p| p.sample_rate())
    }

    fn source_available(&mut self) -> usize {
        self.with(0, |p| p.source_available())
    }

    fn process(&mut self, claimed_source_rate: f64, destination_rate: f64, frames: usize) {
        self.with((), |p| p.process(claimed_source_rate, destination_rate, frames));
    }

    fn available(&self) -> usize {
        self.with(0, |p| p.available())
    }

    fn read(&mut self, out: &mut [i16], frames: usize) -> usize {
        self.with(0, |p| p.read(out, frames))
    }

    fn framerate_ratio(&self, fps_target: f64) -> f64 {
        self.with(1.0, |p| p.framerate_ratio(fps_target))
    }

    fn discard_source(&mut self, frames: usize) {
        self.with((), |p| p.discard_source(frames));
    }
}

impl ReplaySession {
    /// Build a playback session for an SIO-engine replay
    /// ([`tango_replay::VERSION`]): one continuous run of pair
    /// ticks, re-simulated on a linearly-driven pair. Both sides must
    /// have replay support from their engine. Returns
    /// immediately — boot + priming (a second or two) happens on the
    /// drive thread, with a black frame and silence until it's up.
    /// Also returns the session's audio stream (the shown perspective's
    /// core at `sample_rate`, following the drive loop's pacing) for the
    /// host to route to its output.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        games: [&'static tango_gamesupport::Game; 2],
        roms: [Arc<Vec<u8>>; 2],
        replay: Arc<tango_replay::Replay>,
        sample_rate: u32,
        show_pip: bool,
        stats_job: Option<PrefetchStatsJob>,
    ) -> Result<(Self, Workers, crate::audio::CoreStream), crate::Error> {
        let local_player = replay.local_player_index as usize;
        if local_player >= 2 {
            return Err(crate::Error::BadLocalPlayerIndex);
        }
        // The replay's input stream is already absolute pair order
        // (core 0 runs player 0's game) — just widen into the seam's
        // vocabulary.
        let inputs: Arc<Vec<[tango_match::HostInput; 2]>> = Arc::new(
            replay
                .inputs
                .iter()
                .map(|&row| {
                    row.map(|input| tango_match::HostInput {
                        keys: input.keys as u32,
                        touch: input.touch.map(|(x, y)| (x as u16, y as u16)),
                    })
                })
                .collect(),
        );
        let total_ticks = inputs.len() as u32;
        if total_ticks == 0 {
            return Err(crate::Error::EmptyReplay);
        }

        let nickname_of =
            |side: Option<&tango_replay::metadata::Side>| side.map(|s| s.nickname.clone()).unwrap_or_default();
        let input_display = Box::new(InputDisplay {
            pairs: replay
                .inputs
                .iter()
                .map(|&row| {
                    (
                        row[local_player].keys & tango_match::keys::MASK as u16,
                        row[1 - local_player].keys & tango_match::keys::MASK as u16,
                    )
                })
                .collect(),
            nicknames: (nickname_of(replay.local_side()), nickname_of(replay.remote_side())),
        });

        let layout = games[local_player].pvp.screen_layout();
        let screen = crate::Framebuffer::new(&layout);
        let wake = Arc::new(tokio::sync::Notify::new());
        let playback: SharedPlayback = Arc::new(Mutex::new(None));
        let cursor = Arc::new(AtomicU32::new(0));
        let paused = Arc::new(crate::PauseGate::new(false));
        let fps_bits = Arc::new(AtomicU32::new(EXPECTED_FPS.to_bits()));
        let prefetch_progress;
        // Inter-round marks: the recorder stamps round-start markers into
        // the stream and decode surfaces them as `round_starts`. The
        // first round's start (tick 0) isn't an inter-round boundary, so
        // the marks are the rest. Single-round results also cover
        // recordings that predate the markers; for those the prefetch
        // pass re-derives the marks from telemetry as it runs.
        let file_marks: Vec<u32> = replay.round_starts.iter().skip(1).map(|&i| i as u32).collect();
        let discover_marks = file_marks.is_empty();
        let round_marks = Arc::new(Mutex::new(file_marks));
        let seek = Arc::new(SeekController::new());
        let cancel = Arc::new(AtomicBool::new(false));
        let show_pip = Arc::new(AtomicBool::new(show_pip));
        let swap_perspective = Arc::new(AtomicBool::new(false));
        // Which seat is on screen and in the speakers. Kept as a number
        // rather than derived at each use, because the engine's audio
        // pull reads it per fill.
        let shown_seat = Arc::new(AtomicUsize::new(local_player));
        let pip = crate::Framebuffer::new(&layout);
        let pip_fresh = Arc::new(AtomicBool::new(false));

        // The recording as the local game's engine offers it. Nothing
        // is simulated yet: each of the two passes boots on whichever
        // worker the host runs it on, which is what keeps those boots
        // concurrent.
        let set: Arc<dyn tango_match::ReplaySet> = Arc::from(games[local_player].pvp.open_replay(
            tango_match::ReplayConfig {
                roms: [roms[0].to_vec(), roms[1].to_vec()],
                saves: replay.srams.clone(),
                inputs: inputs.clone(),
                rng_seed: replay.rng_seed,
                rtc: replay.rtc_time(),
                match_type: (replay.metadata.match_type as u8, replay.metadata.match_subtype as u8),
                local_player,
                peer_rom: tango_match::PeerRom {
                    code: *games[1 - local_player].rom_code,
                    revision: games[1 - local_player].revision,
                },
                want_stats: stats_job.is_some(),
                want_round_marks: discover_marks,
            },
        )?);
        prefetch_progress = set.stats_progress();
        if let Some(discovered) = set.round_marks() {
            // The pass writes into its own list; hand the session that
            // one so the scrub bar sees marks as they are found.
            *round_marks.lock().unwrap() = discovered.lock().unwrap().clone();
        }
        let audio_pull = DeferredPull::default();

        let surfaces = Surfaces {
            shown_seat: shown_seat.clone(),
            screen: screen.clone(),
            pip: pip.clone(),
            pip_fresh: pip_fresh.clone(),
            show_pip: show_pip.clone(),
            swap_perspective: swap_perspective.clone(),
            wake: wake.clone(),
            local_player,
        };

        // The three loops this session needs run. Which of them get
        // threads is the host's call: a desktop gives each one
        // ([`Workers::split`]), a browser ticks them in turn
        // ([`Workers::into_driver`]).
        let workers = Workers {
            drive: DriveWorker {
                playhead: Playhead {
                    set: set.clone(),
                    playback: playback.clone(),
                    cursor: cursor.clone(),
                    paused: paused.clone(),
                    cancel: cancel.clone(),
                    surfaces: surfaces.clone(),
                    audio: audio_pull.clone(),
                    seat: shown_seat.clone(),
                },
                fps_bits: fps_bits.clone(),
                paused: paused.clone(),
                cancel: cancel.clone(),
                booted: false,
            },
            seek: SeekWorker {
                seek: seek.clone(),
                playback: playback.clone(),
                cursor: cursor.clone(),
                paused: paused.clone(),
                surfaces: surfaces.clone(),
            },
            prefetch: PrefetchWorker {
                set: set.clone(),
                round_marks: discover_marks.then(|| round_marks.clone()),
                discovered: set.round_marks(),
                cancel: cancel.clone(),
                stats_job,
                pass: None,
                finished: None,
                done: false,
            },
        };

        // Audio: play the shown perspective's core straight off the
        // pair, following the drive loop's pacing (see
        // [`crate::core_stream`]).
        let audio = crate::audio::CoreStream::new(
            Box::new(audio_pull) as Box<dyn tango_match::AudioPull>,
            crate::audio::CoreStream::fps_from_bits(fps_bits.clone()),
            sample_rate,
        );

        let session = Self {
            game: games[local_player],
            round_boundaries: round_marks,
            total_ticks,
            input_display,
            layout,
            screen,
            wake,
            show_pip,
            swap_perspective,
            pip,
            pip_fresh,
            engine: Engine {
                local_player,
                cursor,
                paused,
                fps_bits,
                set,
                playback,
                prefetch_progress,
                seek,
                cancel,
            },
        };
        Ok((session, workers, audio))
    }

    /// Whether the opponent-screen PiP is on — drives the transport bar
    /// toggle's lit state.
    pub fn show_pip(&self) -> bool {
        self.show_pip.load(Ordering::Relaxed)
    }

    /// Toggle the opponent-screen PiP. While playing, the overlay
    /// appears on the next published frame; on a paused replay it's
    /// re-blitted from the current frame's snapshot immediately.
    pub fn toggle_pip(&self) {
        self.show_pip.fetch_xor(true, Ordering::Relaxed);
        self.refresh_paused_frame();
    }

    /// Whether the main screen shows the opponent's perspective — drives
    /// the transport bar toggle's lit state.
    pub fn swap_perspective(&self) -> bool {
        self.swap_perspective.load(Ordering::Relaxed)
    }

    /// Swap which perspective the main screen shows. Takes effect on
    /// the next published frame while playing, immediately while paused
    /// — like the PiP.
    pub fn toggle_swap_perspective(&self) {
        self.swap_perspective.fetch_xor(true, Ordering::Relaxed);
        self.refresh_paused_frame();
    }

    /// Re-blit the current frame's snapshot so a perspective toggle
    /// takes effect immediately on a paused replay — the frame callback
    /// won't run to repaint the surfaces until playback resumes.
    /// Reading the shadow's live video buffer instead would be wrong
    /// here: after a zero-frame seek landing the shadow core has loaded
    /// state but never run, so its buffer still holds pre-seek pixels.
    ///
    /// Nearest-within-2 rather than exact: a pause can land after the
    /// next frame's input was consumed but before that frame completed
    /// and published, leaving the playhead one tick ahead of both the
    /// displayed frame and the last capture — an exact lookup misses
    /// there (and the surfaces would silently stay stale, which is the
    /// bug this fixes). The bound keeps a genuine miss (the pre-round
    /// boot window, where no snapshots exist) from jumping the paused
    /// screen to some distant keyframe; those toggles just wait for the
    /// next published frame as before.
    fn refresh_paused_frame(&self) {
        if !self.is_paused() {
            return;
        }
        let tick = self.current_tick();
        if let Some(snap) = self.nearest_snapshot(tick) {
            if snap.frame_index().abs_diff(tick) <= 2 {
                self.blit_snapshot(&snap);
            }
        }
    }

    pub fn is_paused(&self) -> bool {
        self.engine.paused.paused()
    }

    /// Current factor (current fps / 60).
    pub fn speed(&self) -> f32 {
        f32::from_bits(self.engine.fps_bits.load(Ordering::Relaxed)) / EXPECTED_FPS
    }

    /// Toggle playback between paused and running.
    pub fn set_paused(&self, paused: bool) {
        // Unpausing at end-of-stream is a no-op — the drive loop
        // re-pauses before running a frame.
        self.engine.paused.set(paused);
    }

    /// Playhead position on the seek bar: the recorded-frame index =
    /// cumulative input pairs consumed. Freezes during the input-less
    /// inter-round animation (so it rests on the round mark while it
    /// plays), and reaches `total_ticks` exactly when the replay finishes.
    pub fn current_tick(&self) -> u32 {
        self.engine.cursor.load(Ordering::Relaxed)
    }

    pub fn total_ticks(&self) -> u32 {
        self.total_ticks
    }

    /// The recorded (local, remote) joyflags behind the frame at
    /// `tick`. The playhead coordinate counts input pairs consumed,
    /// so the pair that produced tick `t` is index `t - 1`;
    /// all-released at tick 0, before anything has been consumed.
    /// While the playhead is frozen (the input-less inter-round
    /// animation), this holds the round's last pair.
    pub fn input_at(&self, tick: u32) -> (u16, u16) {
        tick.checked_sub(1)
            .and_then(|i| self.input_display.pairs.get(i as usize))
            .copied()
            .unwrap_or((0, 0))
    }

    /// (local, remote) nicknames from the replay metadata — the
    /// input display chips' captions. Either may be empty.
    pub fn nicknames(&self) -> (&str, &str) {
        (&self.input_display.nicknames.0, &self.input_display.nicknames.1)
    }

    /// Highest tick the background prefetcher has reached, for the
    /// progress overlay on the scrub bar. Hits `total_ticks` when the
    /// prefetcher has run to completion.
    pub fn prefetch_progress(&self) -> u32 {
        self.engine.prefetch_progress.load(Ordering::Relaxed)
    }

    /// Recorded-frame index of each inter-round transition — the marks the
    /// scrubber draws. These sit on the same scale as the playhead
    /// ([`Self::current_tick`]), so a mark coincides exactly with the
    /// playhead as it crosses. Trap replays know them up front (the
    /// running sum of round lengths); SIO replays discover them from
    /// telemetry as the prefetch pass runs. Empty for a single-round
    /// replay.
    pub fn round_boundaries(&self) -> Vec<u32> {
        self.round_boundaries.lock().unwrap().clone()
    }

    /// Jump the playhead to `target`, asynchronously. Records the request
    /// on the seek controller and returns immediately; the seek worker
    /// runs the capture load + frame catch-up on its own thread, and
    /// newer requests supersede in-flight ones mid-chase. With
    /// `resume_after`, playback unpauses once the chase lands (unless a
    /// newer request took over) — used by scrub commits, which pause
    /// playback for the duration of the drag.
    pub fn seek_to(&self, target: u32, resume_after: bool) {
        self.seek_ctrl().request(target.min(self.total_ticks), resume_after);
    }

    /// Target of the in-flight seek, if any — lets the UI draw the
    /// playhead where it's headed instead of snapping back to the
    /// pre-seek tick until the chase lands.
    pub fn pending_seek_target(&self) -> Option<u32> {
        self.seek_ctrl().pending_target()
    }

    /// True while an in-flight seek will unpause playback on landing.
    /// The thread is paused for the chase's duration, but the session
    /// is logically still playing — the transport shouldn't flip to
    /// the paused state.
    pub fn seek_will_resume(&self) -> bool {
        self.seek_ctrl().resume_pending()
    }

    /// Withdraw an in-flight seek's pending resume, keeping playback
    /// paused once it lands.
    pub fn cancel_seek_resume(&self) {
        self.seek_ctrl().clear_resume();
    }

    fn seek_ctrl(&self) -> &SeekController {
        &self.engine.seek
    }

    /// The whole-pair snapshot best suited to jump-start a clip export
    /// at playhead tick `start`: the latest capture strictly *before*
    /// it (keyframe store ∪ rewind ring), so the clip's first frame is
    /// still produced by a stepped tick rather than promised from a
    /// framebuffer we can't re-emit. `None` means the export falls
    /// back to simulating from boot.
    pub fn clip_start_tick(&self, start: u32) -> Option<u32> {
        self.engine
            .playback
            .lock()
            .unwrap()
            .as_ref()?
            .capture_before(start)
    }

    /// The captured snapshot nearest `target`, if any — backs the hover
    /// thumbnail above the scrub bar and the drag preview blit. Near the
    /// playhead the rewind window supplies exact frames; elsewhere it's
    /// the store's keyframes.
    pub fn nearest_snapshot(&self, target: u32) -> Option<NearestSnapshot> {
        let guard = self.engine.playback.lock().unwrap();
        let pb = guard.as_ref()?;
        let mut found = None;
        pb.nearest_capture(target, &mut |frames| {
            found = Some(NearestSnapshot {
                tick: frames.tick(),
                frames: [frames.frame(0), frames.frame(1)],
                local_player: self.engine.local_player,
            });
        });
        found
    }

    /// Blit the captured framebuffer of the snapshot nearest `target`
    /// straight into the shared display buffer — instant, emulation-free
    /// feedback while the user drags the scrubber. The exact landing
    /// happens on release via [`Self::seek_to`].
    ///
    /// Unless `force_keyframe`, the blit is skipped while the playhead's
    /// own (exact) frame is at least as close to `target` as the nearest
    /// snapshot — every drag starts by pressing on the handle, and
    /// jumping the display to a keyframe seconds away would glitch.
    /// Once a drag has swapped to keyframes the live frame is no longer
    /// in the buffer, so callers pass `force_keyframe` from then on.
    /// Returns whether a blit happened.
    pub fn scrub_preview(&self, target: u32, force_keyframe: bool) -> bool {
        let Some(snap) = self.nearest_snapshot(target) else {
            return false;
        };
        if !force_keyframe {
            let cur = self.current_tick();
            if cur.abs_diff(target) <= snap.frame_index().abs_diff(target) {
                return false;
            }
        }
        self.blit_snapshot(&snap)
    }

    /// Blit the frame at exactly `target`, if a snapshot holds it —
    /// the scrub *press*'s preview. Unlike [`Self::scrub_preview`] this
    /// never substitutes a nearby keyframe: a click seeks to whatever
    /// tick is under the cursor, and blitting the nearest keyframe
    /// there would flash a wrong frame for the chase's duration before
    /// snapping to the real one. Returns whether a blit happened.
    pub fn scrub_preview_exact(&self, target: u32) -> bool {
        match self.nearest_snapshot(target) {
            Some(snap) if snap.frame_index() == target => self.blit_snapshot(&snap),
            _ => false,
        }
    }

    /// Copy `snap`'s stored framebuffers into the display surfaces —
    /// see [`Surfaces`].
    fn blit_snapshot(&self, snap: &NearestSnapshot) -> bool {
        Surfaces {
            shown_seat: Arc::new(AtomicUsize::new(snap.local_player)),
            screen: self.screen.clone(),
            pip: self.pip.clone(),
            pip_fresh: self.pip_fresh.clone(),
            show_pip: self.show_pip.clone(),
            swap_perspective: self.swap_perspective.clone(),
            wake: self.wake.clone(),
            local_player: snap.local_player,
        }
        .publish_frames(snap);
        true
    }
}

impl crate::Session for ReplaySession {
    fn local_game(&self) -> &'static tango_gamesupport::Game {
        self.game
    }

    fn frame(&self) -> Vec<u8> {
        self.screen.read()
    }

    fn screen_layout(&self) -> tango_match::ScreenLayout {
        self.layout.clone()
    }

    fn wake(&self) -> Arc<tokio::sync::Notify> {
        self.wake.clone()
    }

    /// The opponent's screen, or the local one while swapped — `None`
    /// while the PiP is off or before its first captured frame.
    fn pip_frame(&self) -> Option<Vec<u8>> {
        (self.show_pip.load(Ordering::Relaxed) && self.pip_fresh.load(Ordering::Relaxed)).then(|| self.pip.read())
    }

    /// 0.5 = slow-mo. The SIO drive loop paces itself and publishes
    /// the target for its audio stream.
    fn set_speed(&self, factor: f32) {
        let fps = (EXPECTED_FPS * factor).max(1.0);
        self.engine.fps_bits.store(fps.to_bits(), Ordering::Relaxed);
    }
}

/// A captured playback snapshot — what
/// [`ReplaySession::nearest_snapshot`] hands the scrub/hover UI.
pub struct NearestSnapshot {
    tick: u32,
    /// Both seats' frames as they were captured, RGBA8.
    frames: [Vec<u8>; 2],
    local_player: usize,
}

impl NearestSnapshot {
    /// The captured frame's position on the playhead scale.
    pub fn frame_index(&self) -> u32 {
        self.tick
    }

    /// Stable cache key for the hover thumbnail.
    pub fn key_tick(&self) -> u32 {
        self.tick
    }

    /// The local perspective's pixels, same RGBA8 as
    /// [`Session::frame`](crate::Session::frame). May be empty if the
    /// capture had no rendered frame.
    pub fn local_framebuffer(&self) -> Vec<u8> {
        self.frames[self.local_player].clone()
    }
}

impl tango_match::ReplayFrames for NearestSnapshot {
    fn tick(&self) -> u32 {
        self.tick
    }

    fn frame(&self, player: usize) -> Vec<u8> {
        self.frames[player].clone()
    }
}

/// The display surfaces an SIO playback session publishes into, plus
/// the perspective toggles that pick which core lands where — shared
/// between the drive loop, the seek worker's landing publisher, and
/// paused-frame blits so the paths can't drift (the SIO analogue of
/// [`blit_snapshot_surfaces`]).
#[derive(Clone)]
struct Surfaces {
    /// Mirrors `swap_perspective` as a seat number, for the engine's
    /// audio pull.
    shown_seat: Arc<AtomicUsize>,
    screen: Arc<crate::Framebuffer>,
    pip: Arc<crate::Framebuffer>,
    pip_fresh: Arc<AtomicBool>,
    show_pip: Arc<AtomicBool>,
    swap_perspective: Arc<AtomicBool>,
    wake: Arc<tokio::sync::Notify>,
    local_player: usize,
}

impl Surfaces {
    /// Which seat the main screen currently shows.
    fn shown(&self) -> usize {
        let shown = if self.swap_perspective.load(Ordering::Relaxed) {
            1 - self.local_player
        } else {
            self.local_player
        };
        // The audio pull follows the picture.
        self.shown_seat.store(shown, Ordering::Relaxed);
        shown
    }

    /// Copy a (main, other) frame pair into the surfaces and wake the
    /// renderer. Either side may be absent (`None`/empty) — that surface
    /// keeps its last frame.
    fn publish(&self, main: Option<&[u8]>, other: Option<&[u8]>) {
        if let Some(main) = main {
            self.screen.write(main);
        }
        if self.show_pip.load(Ordering::Relaxed) {
            if let Some(other) = other {
                self.pip.write(other);
                self.pip_fresh.store(true, Ordering::Relaxed);
            }
        } else {
            self.pip_fresh.store(false, Ordering::Relaxed);
        }
        // One wake for the pair, after both surfaces are up.
        self.wake.notify_one();
    }

    /// Publish a capture's frames — live or landed, the engine draws no
    /// distinction and neither does this.
    fn publish_frames(&self, frames: &dyn tango_match::ReplayFrames) {
        let shown = self.shown();
        let main = frames.frame(shown);
        let other = frames.frame(1 - shown);
        let pick = |fb: &[u8]| -> Option<Vec<u8>> { (!fb.is_empty()).then(|| fb.to_vec()) };
        self.publish(pick(&main).as_deref(), pick(&other).as_deref());
    }
}

/// Body of the SIO playback drive thread: boot + prime the pair (the
/// slow part — the session shows black + silence until it's done), then
/// pace the linear re-sim at the published fps target, capturing every
/// tick into the rewind ring (keyframes shared into the store) and
/// publishing frames. Reaching end-of-stream pauses; unpausing there is
/// a no-op until a seek moves the playhead back.
#[allow(clippy::too_many_arguments)]
/// Everything the playback half of a replay session needs, whoever is
/// driving it. A desktop gives each of the session's three concerns a
/// thread of its own (see [`Workers::split`]); a browser has one event
/// loop, so [`Driver`] interleaves them.
struct Playhead {
    set: Arc<dyn tango_match::ReplaySet>,
    playback: SharedPlayback,
    cursor: Arc<AtomicU32>,
    paused: Arc<crate::PauseGate>,
    cancel: Arc<AtomicBool>,
    surfaces: Surfaces,
    /// Filled in once the pair is up — until then the host's stream is
    /// pulling silence off it.
    audio: DeferredPull,
    seat: Arc<AtomicUsize>,
}

impl Playhead {
    /// Boot + prime the display pair and show its first frame. Blocks
    /// for the priming walk — the one part of a session that can't be
    /// sliced, since it runs until the games' own traps say it's there.
    fn boot(&self) -> bool {
        let mut pb = match self.set.playback() {
            Ok(pb) => pb,
            // Torn down mid-prime — the host is waiting on this thread's
            // join, not on a session that will never come up.
            Err(tango_match::Error::Cancelled) => return false,
            Err(e) => {
                log::error!("replay: boot failed: {e:?}");
                return false;
            }
        };
        // Bind the host's stream to the real pair; it has been pulling
        // silence since the session was built.
        if let Some(pull) = pb.audio(self.seat.clone()) {
            self.audio.set(pull);
        }
        // Show the primed first frame while paused-at-start or still
        // spinning up.
        pb.frames(&mut |frames| self.surfaces.publish_frames(frames));
        *self.playback.lock().unwrap() = Some(pb);
        true
    }

    /// Advance the playhead one tick, capturing and publishing it.
    /// `false` once there is nothing left to drive — cancelled, or the
    /// pair went away.
    fn step(&self) -> bool {
        if self.cancel.load(Ordering::Relaxed) {
            return false;
        }
        let mut guard = self.playback.lock().unwrap();
        let Some(pb) = guard.as_mut() else { return false };
        if pb.at_end() {
            self.paused.set(true);
            return true;
        }
        pb.step();
        self.cursor.store(pb.cursor(), Ordering::Relaxed);
        pb.frames(&mut |frames| self.surfaces.publish_frames(frames));
        true
    }
}

/// The playback loop, as something a host drives: boot on the first
/// tick, then advance the playhead one frame per tick at the rate
/// [`Drive::fps_target`] publishes.
///
/// Pausing is the host's too — a paused tick does nothing and says so,
/// so a thread can sleep on the gate and an event loop can just come
/// back later.
pub struct DriveWorker {
    playhead: Playhead,
    fps_bits: Arc<AtomicU32>,
    paused: Arc<crate::PauseGate>,
    cancel: Arc<AtomicBool>,
    booted: bool,
}

impl DriveWorker {
    /// Boot the display pair if this is the first tick. Blocks for the
    /// priming walk; `false` if it failed, which ends the session.
    fn boot_if_needed(&mut self) -> bool {
        if self.booted {
            return true;
        }
        self.booted = true;
        self.playhead.boot()
    }

    /// Whether the user has playback stopped. A host that can park
    /// (a thread) should, rather than spinning on ticks that do nothing.
    pub fn paused(&self) -> bool {
        self.paused.paused()
    }

    /// Park until unpaused, for a host that has a thread to park.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn wait_while_paused(&self) {
        self.paused.wait();
    }
}

impl crate::Drive for DriveWorker {
    fn tick(&mut self) -> bool {
        if self.cancel.load(Ordering::Relaxed) {
            return false;
        }
        if !self.boot_if_needed() {
            return false;
        }
        if self.paused.paused() {
            return true;
        }
        self.playhead.step()
    }

    fn fps_target(&self) -> f32 {
        f32::from_bits(self.fps_bits.load(Ordering::Relaxed)).max(1.0)
    }
}

/// The seek loop: chases the targets the transport bar requests.
pub struct SeekWorker {
    seek: Arc<SeekController>,
    playback: SharedPlayback,
    cursor: Arc<AtomicU32>,
    paused: Arc<crate::PauseGate>,
    surfaces: Surfaces,
}

impl SeekWorker {
    /// Advance an in-flight chase by at most `budget` ticks, starting
    /// one if a request is pending. `true` while a chase is still
    /// walking — a pumped host should come straight back rather than
    /// advancing playback underneath it.
    pub fn step(&self, budget: u32) -> bool {
        let mut guard = self.playback.lock().unwrap();
        let Some(pb) = guard.as_mut() else { return false };
        pb.seek_step(
            &self.seek,
            budget,
            &mut |tick| self.cursor.store(tick, Ordering::Relaxed),
            &mut |frames| self.surfaces.publish_frames(frames),
            &mut || self.paused.set(false),
        ) == tango_match::SeekStep::Working
    }

    /// Park until a seek is requested, or the session shuts down
    /// (`false`). What a host's seek thread waits on between chases; a
    /// pumped host just keeps calling [`SeekWorker::step`], which does
    /// nothing until there's something to chase.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn wait_for_request(&self) -> bool {
        self.seek.wait_for_request()
    }
}

/// The prefetch loop: races its own pair through the whole stream for
/// keyframes, round marks and (when the host asked for them) the
/// match-stats analysis.
pub struct PrefetchWorker {
    set: Arc<dyn tango_match::ReplaySet>,
    /// The session's mark list, when the recording predates the
    /// recorder's own markers and the pass has to find them.
    round_marks: Option<Arc<Mutex<Vec<u32>>>>,
    /// Where the pass writes the marks it finds.
    discovered: Option<Arc<Mutex<Vec<u32>>>>,
    cancel: Arc<AtomicBool>,
    stats_job: Option<PrefetchStatsJob>,
    pass: Option<Box<dyn tango_match::StatsPass>>,
    /// What the pass produced, kept so a host can cache it after the
    /// loop rather than racing the deliver.
    finished: Option<tango_match::analysis::MatchStats>,
    /// The pass finished, failed, or was cancelled — don't reopen it.
    done: bool,
}

impl PrefetchWorker {
    /// Advance the pass, opening it on the first call. `false` once the
    /// pass is done (or was never wanted).
    ///
    /// Deliberately the host's to schedule: this is background work
    /// competing with playback for the same thread, so *when* and *how
    /// much* is its call — a browser has to fit it into whatever the
    /// frame left over, and getting that wrong pegs the event loop.
    pub fn step(&mut self, budget: u32) -> bool {
        if self.done {
            return false;
        }
        if self.cancel.load(Ordering::Relaxed) {
            self.done = true;
            return false;
        }
        if self.pass.is_none() {
            match self.set.stats() {
                Ok(pass) => self.pass = Some(pass),
                Err(tango_match::Error::Cancelled) => {
                    self.done = true;
                    return false;
                }
                Err(e) => {
                    log::error!("replay prefetch failed to open: {e:?}");
                    self.done = true;
                    return false;
                }
            }
        }
        let Some(pass) = self.pass.as_mut() else {
            return false;
        };
        match pass.step(budget) {
            Ok(true) => {
                self.mirror_marks();
                true
            }
            Ok(false) => {
                self.mirror_marks();
                let stats = self.pass.take().and_then(|p| p.finish());
                self.finished = stats.clone();
                self.deliver(stats);
                self.done = true;
                false
            }
            Err(e) => {
                log::error!("replay prefetch failed: {e:?}");
                self.pass = None;
                self.done = true;
                false
            }
        }
    }

    /// Copy whatever marks the pass has found into the session's list,
    /// so the scrub bar picks them up while the pass is still running.
    fn mirror_marks(&self) {
        let (Some(into), Some(from)) = (self.round_marks.as_ref(), self.discovered.as_ref()) else {
            return;
        };
        let found = from.lock().unwrap().clone();
        *into.lock().unwrap() = found;
    }

    /// The fold so far, for a host previewing the chart mid-pass.
    pub fn preview(&self) -> Option<tango_match::analysis::MatchStats> {
        self.pass.as_ref()?.preview()
    }

    /// The finished statistics, once [`step`](Self::step) has reported
    /// the pass over. The host writes the cache — a desktop has
    /// somewhere to put it and a browser doesn't.
    pub fn finished(&self) -> Option<tango_match::analysis::MatchStats> {
        self.finished.clone()
    }

    /// The stats job this pass was opened for, if any — the host
    /// delivers to it when the pass finishes, and previews into it as
    /// the fold runs.
    pub fn stats_job(&self) -> Option<&PrefetchStatsJob> {
        self.stats_job.as_ref()
    }

    /// Hand a finished pass's stats to that job. The cache write is the
    /// host's on a desktop; a browser has nowhere to put it.
    fn deliver(&self, stats: Option<tango_match::analysis::MatchStats>) {
        let (Some(stats), Some(job)) = (stats, self.stats_job.as_ref()) else {
            return;
        };
        *job.done.lock().unwrap() = Some(stats);
    }
}

/// The three loops a replay session needs run, before anyone has
/// decided what runs them.
pub struct Workers {
    pub drive: DriveWorker,
    pub seek: SeekWorker,
    pub prefetch: PrefetchWorker,
}

impl Workers {
    /// Take the three loops apart, for a host that gives each a thread.
    pub fn split(self) -> (DriveWorker, SeekWorker, PrefetchWorker) {
        (self.drive, self.seek, self.prefetch)
    }

    /// Fold them into one driver, for a host with a single event loop.
    pub fn into_driver(self) -> Driver {
        Driver {
            drive: self.drive,
            seek: self.seek,
            prefetch: self.prefetch,
        }
    }
}

/// All three replay loops on one thread of control, a slice at a time.
///
/// A seek in flight owns the tick — advancing playback underneath a
/// chase would fight it — and otherwise the playhead advances. The
/// prefetch pass is *not* in here: it's background work on the same
/// thread, so the host schedules it with
/// [`Driver::prefetch_step`] out of whatever the frame left over.
pub struct Driver {
    drive: DriveWorker,
    seek: SeekWorker,
    prefetch: PrefetchWorker,
}

impl Driver {
    /// Ticks of chase per pumped frame. A seek is the user waiting, so
    /// it gets the most.
    const SEEK_SLICE: u32 = 64;

    /// Advance the prefetch pass, opening its pair on the first call.
    /// `false` once the pass is done (or was never wanted).
    ///
    /// Deliberately not part of [`Drive::tick`]: this is background work
    /// that competes with playback for the same thread, so *when* and
    /// *how much* is the host's to decide — a browser has to fit it into
    /// whatever the frame left over, and getting that wrong pegs the
    /// event loop.
    pub fn prefetch_step(&mut self, budget: u32) -> bool {
        self.prefetch.step(budget)
    }
}

impl crate::Drive for Driver {
    fn tick(&mut self) -> bool {
        if self.seek.step(Self::SEEK_SLICE) {
            // A chase is mid-walk: it owns this tick.
            return true;
        }
        self.drive.tick()
    }

    fn fps_target(&self) -> f32 {
        self.drive.fps_target()
    }
}

pub struct PrefetchStatsJob {
    pub partial_tx: futures::channel::mpsc::UnboundedSender<tango_match::analysis::MatchStats>,
    pub done: Arc<Mutex<Option<tango_match::analysis::MatchStats>>>,
    pub stats_file: std::path::PathBuf,
}
