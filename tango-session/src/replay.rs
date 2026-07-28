//! Replay playback session: a linearly-driven
//! [`tango_match_mgba::playback::Playback`] pair behind a mutex, paced by
//! a drive thread; a prefetch pair races ahead of the playhead filling
//! a keyframe [`SnapshotStore`] (and doubling as the match-stats
//! analysis), and a [`RewindRing`] keeps every tick of the last ~1.5s
//! so single-frame backward steps land on exact snapshots. Seeks are
//! asynchronous: requests land on a [`SeekController`] and a dedicated
//! worker chases the newest target, so the UI never blocks on catch-up
//! emulation. Audio is pulled straight off the pair via
//! [`crate::audio::CoreStream`].
//!
//! [`SnapshotStore`]: tango_match_mgba::playback::SnapshotStore
//! [`RewindRing`]: tango_match_mgba::playback::RewindRing

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use tango_match_mgba::playback::SeekController;

pub const SCREEN_WIDTH: u32 = mgba::gba::SCREEN_WIDTH;
pub const SCREEN_HEIGHT: u32 = mgba::gba::SCREEN_HEIGHT;
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

/// SIO-engine playback: a linearly-driven [`Playback`] pair behind a
/// mutex, paced by a host drive thread; the seek worker chases targets
/// by loading the nearest pair snapshot and stepping forward, and the
/// prefetch worker races its own pair ahead for keyframes + stats +
/// round marks (see [`tango_match_mgba::playback`]).
///
/// [`Playback`]: tango_match_mgba::playback::Playback
struct Engine {
    /// Which pair core is the replay's local perspective.
    local_player: usize,
    /// Lock-free playhead mirror for UI reads.
    cursor: Arc<AtomicU32>,
    paused: Arc<crate::PauseGate>,
    /// Pacing target, f32 bits (60 × speed factor).
    fps_bits: Arc<AtomicU32>,
    snapshots: tango_match_mgba::playback::SnapshotStore,
    rewind: tango_match_mgba::playback::RewindRing,
    prefetch_progress: Arc<AtomicU32>,
    seek: Arc<SeekController>,
    /// Cancels the loops the host is running, on Drop; whoever is
    /// running them notices on their next tick (and a host with threads
    /// joins them afterwards).
    cancel: Arc<AtomicBool>,
}

type SharedSioPlayback = Arc<Mutex<Option<tango_match_mgba::playback::Playback>>>;

impl Drop for Engine {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        // Release a gate-parked drive loop so the host's join is prompt.
        self.paused.set(false);
        self.seek.shutdown();
    }
}

/// Cross-thread audio pull over the playback pair's mutex. Uses
/// `try_lock`: on contention (a seek chase holding the pair for its
/// catch-up run) the callback plays silence instead of stalling the
/// audio thread — the chase clears the fast-forward burst when it
/// lands anyway.
struct SioPlaybackPull(SharedSioPlayback);

impl crate::audio::PairPull for SioPlaybackPull {
    fn with_pair(&self, f: &mut dyn FnMut(&mut tango_match_mgba::Link)) {
        if let Ok(mut guard) = self.0.try_lock() {
            if let Some(pb) = guard.as_mut() {
                f(pb.pair_mut());
            }
        }
    }
}

impl ReplaySession {
    /// Build a playback session for an SIO-engine replay
    /// ([`tango_replay::VERSION`]): one continuous run of pair
    /// ticks, re-simulated on a linearly-driven pair. Both sides must
    /// have [`GameSupport`](tango_match_mgba::GameSupport) support. Returns
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
        use tango_match_mgba::playback as sio_playback;

        let local_player = replay.local_player_index as usize;
        if local_player >= 2 {
            return Err(crate::Error::BadLocalPlayerIndex);
        }
        // The replay's input stream is already absolute pair order
        // (core 0 runs player 0's game) — just widen.
        let inputs: Arc<Vec<[u32; 2]>> =
            Arc::new(replay.inputs.iter().map(|&[p1, p2]| [p1 as u32, p2 as u32]).collect());
        let total_ticks = inputs.len() as u32;
        if total_ticks == 0 {
            return Err(crate::Error::EmptyReplay);
        }
        let boot = {
            let replay = replay.clone();
            let roms = roms.clone();
            move || -> sio_playback::BootConfig {
                sio_playback::BootConfig {
                    roms: [roms[0].to_vec(), roms[1].to_vec()],
                    saves: replay.srams.clone(),
                    support: [games[0].pvp, games[1].pvp],
                    match_type: (replay.metadata.match_type as u8, replay.metadata.match_subtype as u8),
                    rng_seed: replay.rng_seed,
                    rtc: replay.rtc_time(),
                    // The viewer always plays the games' own audio; the
                    // BGM-disable knob is the export path's.
                    disable_bgm: false,
                }
            }
        };

        let nickname_of =
            |side: Option<&tango_replay::metadata::Side>| side.map(|s| s.nickname.clone()).unwrap_or_default();
        let input_display = Box::new(InputDisplay {
            pairs: replay
                .inputs
                .iter()
                .map(|&keys| {
                    (
                        keys[local_player] & tango_match::input::JOYFLAGS_MASK,
                        keys[1 - local_player] & tango_match::input::JOYFLAGS_MASK,
                    )
                })
                .collect(),
            nicknames: (nickname_of(replay.local_side()), nickname_of(replay.remote_side())),
        });

        let screen = crate::Framebuffer::new();
        let wake = Arc::new(tokio::sync::Notify::new());
        let playback: SharedSioPlayback = Arc::new(Mutex::new(None));
        let cursor = Arc::new(AtomicU32::new(0));
        let paused = Arc::new(crate::PauseGate::new(false));
        let fps_bits = Arc::new(AtomicU32::new(EXPECTED_FPS.to_bits()));
        let snapshots = sio_playback::SnapshotStore::new();
        let rewind = sio_playback::RewindRing::new();
        let prefetch_progress = Arc::new(AtomicU32::new(0));
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
        let pip = crate::Framebuffer::new();
        let pip_fresh = Arc::new(AtomicBool::new(false));

        let surfaces = Surfaces {
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
                boot_config: boot(),
                inputs: inputs.clone(),
                playhead: Playhead {
                    playback: playback.clone(),
                    cursor: cursor.clone(),
                    paused: paused.clone(),
                    snapshots: snapshots.clone(),
                    rewind: rewind.clone(),
                    cancel: cancel.clone(),
                    surfaces: surfaces.clone(),
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
                snapshots: snapshots.clone(),
                rewind: rewind.clone(),
                surfaces: surfaces.clone(),
            },
            prefetch: PrefetchWorker {
                boot_config: boot(),
                inputs: inputs.clone(),
                local_player,
                snapshots: snapshots.clone(),
                progress: prefetch_progress.clone(),
                round_marks: discover_marks.then(|| round_marks.clone()),
                cancel: cancel.clone(),
                stats: stats_job.as_ref().map(|_| {
                    (
                        games[local_player].pvp.chip_semantics(roms[local_player].as_ref()),
                        games[local_player].pvp.counts_buster(roms[local_player].as_ref()),
                    )
                }),
                stats_job,
            },
        };

        // Audio: play the shown perspective's core straight off the
        // pair, following the drive loop's pacing (see
        // [`crate::core_stream`]).
        let audio = crate::audio::CoreStream::new(
            crate::audio::PairCorePull {
                pair: SioPlaybackPull(playback.clone()),
                player: {
                    let swap_perspective = swap_perspective.clone();
                    Box::new(move || {
                        if swap_perspective.load(Ordering::Relaxed) {
                            1 - local_player
                        } else {
                            local_player
                        }
                    })
                },
            },
            crate::audio::CoreStream::fps_from_bits(fps_bits.clone()),
            sample_rate,
        );

        let session = Self {
            game: games[local_player],
            round_boundaries: round_marks,
            total_ticks,
            input_display,
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
                snapshots,
                rewind,
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
    /// runs the snapshot load + frame catch-up on the mgba thread, and
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
    pub fn clip_start_snapshot(&self, start: u32) -> Option<Arc<tango_match_mgba::playback::Snapshot>> {
        let before = start.checked_sub(1)?;
        let s = &self.engine;
        [
            s.snapshots.best_at_or_before(before),
            s.rewind.best_at_or_before(before),
        ]
        .into_iter()
        .flatten()
        .max_by_key(|s| s.tick)
    }

    /// The captured snapshot nearest `target`, if any — backs the hover
    /// thumbnail above the scrub bar and the drag preview blit. Near the
    /// playhead the rewind window supplies exact frames; elsewhere it's
    /// the store's keyframes.
    pub fn nearest_snapshot(&self, target: u32) -> Option<NearestSnapshot> {
        let s = &self.engine;
        [s.snapshots.nearest(target), s.rewind.nearest(target)]
            .into_iter()
            .flatten()
            .min_by_key(|s| s.tick.abs_diff(target))
            .map(|snap| NearestSnapshot {
                snap,
                local_player: s.local_player,
            })
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
            screen: self.screen.clone(),
            pip: self.pip.clone(),
            pip_fresh: self.pip_fresh.clone(),
            show_pip: self.show_pip.clone(),
            swap_perspective: self.swap_perspective.clone(),
            wake: self.wake.clone(),
            local_player: snap.local_player,
        }
        .publish_snapshot(&snap.snap);
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
    snap: Arc<tango_match_mgba::playback::Snapshot>,
    local_player: usize,
}

impl NearestSnapshot {
    /// The captured frame's position on the playhead scale.
    pub fn frame_index(&self) -> u32 {
        self.snap.tick
    }

    /// Stable cache key for the hover thumbnail.
    pub fn key_tick(&self) -> u32 {
        self.snap.tick
    }

    /// The local perspective's pixels, expanded to RGBA8 like
    /// [`Session::frame`](crate::Session::frame). May be empty if the
    /// capture had no rendered frame.
    pub fn local_framebuffer(&self) -> Vec<u8> {
        let fb = &self.snap.framebuffers[self.local_player];
        let mut rgba = vec![0u8; fb.len() * 2];
        mgba::gba::bgr555_to_rgba8(fb, &mut rgba);
        rgba
    }
}

/// The display surfaces an SIO playback session publishes into, plus
/// the perspective toggles that pick which core lands where — shared
/// between the drive loop, the seek worker's landing publisher, and
/// paused-frame blits so the paths can't drift (the SIO analogue of
/// [`blit_snapshot_surfaces`]).
#[derive(Clone)]
struct Surfaces {
    screen: Arc<crate::Framebuffer>,
    pip: Arc<crate::Framebuffer>,
    pip_fresh: Arc<AtomicBool>,
    show_pip: Arc<AtomicBool>,
    swap_perspective: Arc<AtomicBool>,
    wake: Arc<tokio::sync::Notify>,
    local_player: usize,
}

impl Surfaces {
    /// Which pair core the main screen currently shows.
    fn shown(&self) -> usize {
        if self.swap_perspective.load(Ordering::Relaxed) {
            1 - self.local_player
        } else {
            self.local_player
        }
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

    /// Publish the pair's live framebuffers.
    fn publish_pair(&self, pair: &mut tango_match_mgba::Link) {
        let shown = self.shown();
        let main = pair.video_buffer(shown).map(|b| b.to_vec());
        let other = pair.video_buffer(1 - shown).map(|b| b.to_vec());
        self.publish(main.as_deref(), other.as_deref());
    }

    /// Publish a captured snapshot's framebuffers (emulation-free).
    fn publish_snapshot(&self, snap: &tango_match_mgba::playback::Snapshot) {
        let shown = self.shown();
        let pick = |i: usize| -> Option<&[u8]> {
            let fb = snap.framebuffers[i].as_slice();
            (!fb.is_empty()).then_some(fb)
        };
        self.publish(pick(shown), pick(1 - shown));
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
    playback: SharedSioPlayback,
    cursor: Arc<AtomicU32>,
    paused: Arc<crate::PauseGate>,
    snapshots: tango_match_mgba::playback::SnapshotStore,
    rewind: tango_match_mgba::playback::RewindRing,
    cancel: Arc<AtomicBool>,
    surfaces: Surfaces,
}

impl Playhead {
    /// Boot + prime the display pair and show its first frame. Blocks
    /// for the priming walk — the one part of a session that can't be
    /// sliced, since it runs until the games' own traps say it's there.
    fn boot(&self, boot_config: &tango_match_mgba::playback::BootConfig, inputs: Arc<Vec<[u32; 2]>>) -> bool {
        // The display pair runs no telemetry observer — its lifecycle
        // sink is a write-only stub.
        let pb = match tango_match_mgba::playback::Playback::new(
            boot_config,
            inputs,
            &tango_match_mgba::telemetry::LifecycleSink::new(),
        ) {
            Ok(pb) => pb,
            Err(e) => {
                log::error!("sio replay: boot failed: {e:?}");
                return false;
            }
        };
        *self.playback.lock().unwrap() = Some(pb);
        // Show the primed first frame while paused-at-start or still
        // spinning up.
        if let Some(pb) = self.playback.lock().unwrap().as_mut() {
            self.surfaces.publish_pair(pb.pair_mut());
        }
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
        match pb.capture() {
            Ok(snap) => {
                if self.snapshots.snapshot_needed(snap.tick) {
                    self.snapshots.push(snap.clone());
                }
                self.rewind.insert(snap);
            }
            Err(e) => log::warn!("sio replay: frame capture failed: {e:?}"),
        }
        self.surfaces.publish_pair(pb.pair_mut());
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
    boot_config: tango_match_mgba::playback::BootConfig,
    inputs: Arc<Vec<[u32; 2]>>,
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
        self.playhead.boot(&self.boot_config, self.inputs.clone())
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
    seek: Arc<tango_match_mgba::playback::SeekController>,
    playback: SharedSioPlayback,
    cursor: Arc<AtomicU32>,
    paused: Arc<crate::PauseGate>,
    snapshots: tango_match_mgba::playback::SnapshotStore,
    rewind: tango_match_mgba::playback::RewindRing,
    surfaces: Surfaces,
}

impl SeekWorker {
    /// Advance an in-flight chase by at most `budget` ticks, starting
    /// one if a request is pending. `true` while a chase is still
    /// walking — a pumped host should come straight back rather than
    /// advancing playback underneath it.
    pub fn step(&self, chase: &mut tango_match_mgba::playback::SeekChase, budget: u32) -> bool {
        chase.step(
            &self.seek,
            &self.playback,
            &self.snapshots,
            &self.rewind,
            budget,
            &mut |tick| self.cursor.store(tick, Ordering::Relaxed),
            &mut |snap| self.surfaces.publish_snapshot(snap),
            &mut || self.paused.set(false),
        ) == tango_match_mgba::playback::ChaseStep::Working
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
    boot_config: tango_match_mgba::playback::BootConfig,
    inputs: Arc<Vec<[u32; 2]>>,
    local_player: usize,
    snapshots: tango_match_mgba::playback::SnapshotStore,
    progress: Arc<AtomicU32>,
    round_marks: Option<Arc<Mutex<Vec<u32>>>>,
    cancel: Arc<AtomicBool>,
    stats: Option<(tango_match_mgba::analysis::ChipSemantics, bool)>,
    stats_job: Option<PrefetchStatsJob>,
}

impl PrefetchWorker {
    /// Open the prefetch pair. Blocks for its priming walk — the one
    /// part of a pass that can't be sliced.
    pub fn open(&self) -> Result<tango_match_mgba::playback::Prefetch, tango_match_mgba::Error> {
        tango_match_mgba::playback::Prefetch::open(
            &self.boot_config,
            self.inputs.clone(),
            self.local_player,
            self.snapshots.clone(),
            self.progress.clone(),
            self.round_marks.clone(),
            self.cancel.clone(),
            self.stats,
        )
    }

    /// The stats job this pass was opened for, if any — the host
    /// delivers to it when the pass finishes, and previews into it as
    /// the fold runs.
    pub fn stats_job(&self) -> Option<&PrefetchStatsJob> {
        self.stats_job.as_ref()
    }

    /// Hand a finished pass's stats to that job. The cache write is the
    /// host's on a desktop; a browser has nowhere to put it.
    fn deliver(&self, stats: Option<tango_match_mgba::analysis::MatchStats>) {
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
            chase: tango_match_mgba::playback::SeekChase::default(),
            prefetch: self.prefetch,
            pass: None,
            done_prefetching: false,
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
    chase: tango_match_mgba::playback::SeekChase,
    prefetch: PrefetchWorker,
    pass: Option<tango_match_mgba::playback::Prefetch>,
    /// The pass finished, failed, or was cancelled — don't reopen it.
    done_prefetching: bool,
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
        if self.pass.is_none() {
            if self.done_prefetching {
                return false;
            }
            match self.prefetch.open() {
                Ok(pass) => self.pass = Some(pass),
                Err(tango_match_mgba::Error::Cancelled) => {
                    self.done_prefetching = true;
                    return false;
                }
                Err(e) => {
                    log::error!("sio replay prefetch failed to open: {e:?}");
                    self.done_prefetching = true;
                    return false;
                }
            }
        }
        let Some(pass) = self.pass.as_mut() else {
            return false;
        };
        match pass.step(budget, None) {
            Ok(true) => true,
            Ok(false) => {
                let stats = self.pass.take().and_then(|p| p.finish());
                self.prefetch.deliver(stats);
                self.done_prefetching = true;
                false
            }
            Err(e) => {
                log::error!("sio replay prefetch failed: {e:?}");
                self.pass = None;
                self.done_prefetching = true;
                false
            }
        }
    }
}

impl crate::Drive for Driver {
    fn tick(&mut self) -> bool {
        if self.seek.step(&mut self.chase, Self::SEEK_SLICE) {
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
    pub partial_tx: futures::channel::mpsc::UnboundedSender<tango_match_mgba::analysis::MatchStats>,
    pub done: Arc<Mutex<Option<tango_match_mgba::analysis::MatchStats>>>,
    pub stats_file: std::path::PathBuf,
}
