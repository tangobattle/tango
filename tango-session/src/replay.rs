//! Replay playback session: a linearly-driven
//! [`tango_match::playback::Playback`] pair behind a mutex, paced by
//! a drive thread; a prefetch pair races ahead of the playhead filling
//! a keyframe [`SnapshotStore`] (and doubling as the match-stats
//! analysis), and a [`RewindRing`] keeps every tick of the last ~1.5s
//! so single-frame backward steps land on exact snapshots. Seeks are
//! asynchronous: requests land on a [`SeekController`] and a dedicated
//! worker chases the newest target, so the UI never blocks on catch-up
//! emulation. Audio is pulled straight off the pair via
//! [`crate::core_stream::CoreStream`].
//!
//! [`SnapshotStore`]: tango_match::playback::SnapshotStore
//! [`RewindRing`]: tango_match::playback::RewindRing

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use tango_match::playback::SeekController;

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
    /// the `ActiveSession` enum — small, same as the PvP variant.
    input_display: Box<InputDisplay>,
    /// This session's display framebuffer + wake handle, kept so
    /// [`Self::scrub_preview`] can blit snapshot framebuffers without
    /// going through the emulator at all.
    frame_sink: crate::FrameSink,
    /// Whether the opponent-screen PiP is on (a per-session toggle on
    /// the transport bar).
    show_pip: Arc<AtomicBool>,
    /// Whether the main screen shows the opponent's perspective instead
    /// of the local one — a per-session toggle on the transport bar. The
    /// PiP, when also on, carries the local screen so the two surfaces
    /// always show both sides.
    swap_perspective: Arc<AtomicBool>,
    /// The opponent's screen, copied once per published frame while the
    /// PiP is on. Same BGR555 layout as `vbuf`.
    pip_vbuf: Arc<Mutex<Vec<u8>>>,
    /// Whether `pip_vbuf` holds a frame from the current PiP activation
    /// (cleared while off, so a stale capture never flashes on re-toggle).
    pip_fresh: Arc<AtomicBool>,
    /// The playback machinery (pair, workers, seek state).
    engine: Engine,
}

/// SIO-engine playback: a linearly-driven [`Playback`] pair behind a
/// mutex, paced by a host drive thread; the seek worker chases targets
/// by loading the nearest pair snapshot and stepping forward, and the
/// prefetch worker races its own pair ahead for keyframes + stats +
/// round marks (see [`tango_match::playback`]).
///
/// [`Playback`]: tango_match::playback::Playback
struct Engine {
    /// Which pair core is the replay's local perspective.
    local_player: usize,
    /// Lock-free playhead mirror for UI reads.
    cursor: Arc<AtomicU32>,
    paused: Arc<crate::PauseGate>,
    /// Pacing target, f32 bits (60 × speed factor).
    fps_bits: Arc<AtomicU32>,
    snapshots: tango_match::playback::SnapshotStore,
    rewind: tango_match::playback::RewindRing,
    prefetch_progress: Arc<AtomicU32>,
    seek: Arc<SeekController>,
    /// Cancels the drive + prefetch threads on Drop.
    cancel: Arc<AtomicBool>,
    /// Joined on Drop, after `cancel` and the seek controller's
    /// shutdown, so no thread outlives the surfaces they publish to.
    threads: Vec<std::thread::JoinHandle<()>>,
}

type SharedSioPlayback = Arc<Mutex<Option<tango_match::playback::Playback>>>;

impl Drop for Engine {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        // Release a gate-parked drive thread so the join below is prompt.
        self.paused.set(false);
        self.seek.shutdown();
        for h in self.threads.drain(..) {
            let _ = h.join();
        }
    }
}

/// Cross-thread audio pull over the playback pair's mutex. Uses
/// `try_lock`: on contention (a seek chase holding the pair for its
/// catch-up run) the callback plays silence instead of stalling the
/// audio thread — the chase clears the fast-forward burst when it
/// lands anyway.
struct SioPlaybackPull(SharedSioPlayback);

impl crate::core_stream::PairPull for SioPlaybackPull {
    fn with_pair(&self, f: &mut dyn FnMut(&mut tango_match::Link)) {
        if let Ok(mut guard) = self.0.try_lock() {
            if let Some(pb) = guard.as_mut() {
                f(pb.pair_mut());
            }
        }
    }
}

impl ReplaySession {
    /// Build a playback session for an SIO-engine replay
    /// ([`tango_match::replay::VERSION`]): one continuous run of pair
    /// ticks, re-simulated on a linearly-driven pair. Both sides must
    /// have [`GameSupport`](tango_match::GameSupport) support. Returns
    /// immediately — boot + priming (a second or two) happens on the
    /// drive thread, with a black frame and silence until it's up.
    /// Also returns the session's audio stream (the shown perspective's
    /// core at `sample_rate`, following the drive loop's pacing) for the
    /// host to route to its output.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        games: [&'static tango_gamesupport::Game; 2],
        roms: [Arc<Vec<u8>>; 2],
        replay: Arc<tango_match::replay::Replay>,
        sample_rate: u32,
        show_pip: bool,
        stats_job: Option<PrefetchStatsJob>,
    ) -> Result<(Self, crate::core_stream::CoreStream), crate::Error> {
        use tango_match::playback as sio_playback;

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

        let nickname_of = |side: Option<&tango_match::replay::metadata::Side>| {
            side.map(|s| s.nickname.clone()).unwrap_or_default()
        };
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
            nicknames: (
                nickname_of(replay.local_side()),
                nickname_of(replay.remote_side()),
            ),
        });

        let frame_sink = crate::FrameSink::new();
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
        let pip_vbuf = Arc::new(Mutex::new(vec![
            0u8;
            (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT * 2)
                as usize
        ]));
        let pip_fresh = Arc::new(AtomicBool::new(false));

        let surfaces = Surfaces {
            vbuf: frame_sink.vbuf.clone(),
            pip_vbuf: pip_vbuf.clone(),
            pip_fresh: pip_fresh.clone(),
            show_pip: show_pip.clone(),
            swap_perspective: swap_perspective.clone(),
            frame_notify: frame_sink.notify.clone(),
            local_player,
        };

        let mut threads = Vec::new();

        // The drive thread: boots + primes the pair, then paces the
        // linear re-sim at the published fps target, capturing every
        // tick into the rewind ring (keyframes shared into the store)
        // and publishing frames.
        threads.push(
            std::thread::Builder::new()
                .name("tango-sio-replay-drive".to_owned())
                .spawn({
                    let boot_config = boot();
                    let inputs = inputs.clone();
                    let playback = playback.clone();
                    let cursor = cursor.clone();
                    let paused = paused.clone();
                    let fps_bits = fps_bits.clone();
                    let snapshots = snapshots.clone();
                    let rewind = rewind.clone();
                    let cancel = cancel.clone();
                    let surfaces = surfaces.clone();
                    move || {
                        run_drive(
                            boot_config,
                            inputs,
                            playback,
                            cursor,
                            paused,
                            fps_bits,
                            snapshots,
                            rewind,
                            cancel,
                            surfaces,
                        )
                    }
                })?,
        );

        // The prefetch worker: races its own pair through the whole
        // stream for keyframes, round marks, and (optionally) the
        // match-stats analysis — the SIO analogue of [`Prefetcher`].
        threads.push(
            std::thread::Builder::new()
                .name("tango-sio-replay-prefetch".to_owned())
                .spawn({
                    let boot_config = boot();
                    let inputs = inputs.clone();
                    let snapshots = snapshots.clone();
                    let prefetch_progress = prefetch_progress.clone();
                    let round_marks = discover_marks.then(|| round_marks.clone());
                    let cancel = cancel.clone();
                    let chip_semantics = games[local_player].pvp.chip_semantics(roms[local_player].as_ref());
                    let counts_buster = games[local_player].pvp.counts_buster(roms[local_player].as_ref());
                    move || {
                        const PREVIEW_EVERY: std::time::Duration = std::time::Duration::from_millis(33);
                        let mut last_preview = std::time::Instant::now();
                        let mut on_stats_progress =
                            |_tick: u32, _total: u32, builder: &tango_match::analysis::StatsBuilder| {
                                let Some(job) = &stats_job else { return };
                                let now = std::time::Instant::now();
                                if now.duration_since(last_preview) < PREVIEW_EVERY {
                                    return;
                                }
                                last_preview = now;
                                let _ = job.partial_tx.unbounded_send(builder.preview());
                            };
                        match sio_playback::run_prefetch(
                            &boot_config,
                            &inputs,
                            local_player,
                            snapshots,
                            prefetch_progress,
                            round_marks,
                            cancel,
                            stats_job.is_some().then_some((
                                chip_semantics,
                                counts_buster,
                                &mut on_stats_progress as &mut dyn FnMut(u32, u32, &tango_match::analysis::StatsBuilder),
                            )),
                        ) {
                            Ok(Some(stats)) => {
                                if let Some(job) = &stats_job {
                                    if let Err(e) = crate::stats_cache::write_match_stats(&job.stats_file, &stats)
                                    {
                                        log::warn!("prefetch stats cache write failed: {e:?}");
                                    }
                                    *job.done.lock().unwrap() = Some(stats);
                                }
                            }
                            Ok(None) => {}
                            // Session closed while the prefetch pair was
                            // still priming — a normal teardown, not noise
                            // for the error log.
                            Err(tango_match::Error::Cancelled) => {}
                            Err(e) => log::error!("sio replay prefetch worker exited with error: {e:?}"),
                        }
                    }
                })?,
        );

        // The seek worker: chases seek targets on the playback pair.
        threads.push(
            std::thread::Builder::new()
                .name("tango-sio-replay-seek".to_owned())
                .spawn({
                    let seek = seek.clone();
                    let playback = playback.clone();
                    let cursor = cursor.clone();
                    let paused = paused.clone();
                    let snapshots = snapshots.clone();
                    let rewind = rewind.clone();
                    let surfaces = surfaces.clone();
                    move || {
                        tango_match::playback::run_seek_worker(
                            &seek,
                            &playback,
                            &snapshots,
                            &rewind,
                            &mut |tick| cursor.store(tick, Ordering::Relaxed),
                            &mut |snap| {
                                surfaces.publish_snapshot(snap);
                            },
                            &mut || paused.set(false),
                        );
                    }
                })?,
        );

        // Audio: play the shown perspective's core straight off the
        // pair, following the drive loop's pacing (see
        // [`crate::core_stream`]).
        let audio = crate::core_stream::CoreStream::new(
            crate::core_stream::PairCorePull {
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
            crate::core_stream::CoreStream::fps_from_bits(fps_bits.clone()),
            sample_rate,
        );

        let session = Self {
            game: games[local_player],
            round_boundaries: round_marks,
            total_ticks,
            input_display,
            frame_sink,
            show_pip,
            swap_perspective,
            pip_vbuf,
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
                threads,
            },
        };
        Ok((session, audio))
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
    pub fn clip_start_snapshot(&self, start: u32) -> Option<Arc<tango_match::playback::Snapshot>> {
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
    /// the store's keyframes. Framebuffers are mgba-native BGR555, same
    /// as the shared display buffer.
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
            vbuf: self.frame_sink.vbuf.clone(),
            pip_vbuf: self.pip_vbuf.clone(),
            pip_fresh: self.pip_fresh.clone(),
            show_pip: self.show_pip.clone(),
            swap_perspective: self.swap_perspective.clone(),
            frame_notify: self.frame_sink.notify.clone(),
            local_player: snap.local_player,
        }
        .publish_snapshot(&snap.snap);
        true
    }
}

impl crate::ActiveSession for ReplaySession {
    fn local_game(&self) -> &'static tango_gamesupport::Game {
        self.game
    }

    fn frame_sink(&self) -> &crate::FrameSink {
        &self.frame_sink
    }

    /// The opponent's screen, or the local one while swapped — `None`
    /// while the PiP is off or before its first captured frame.
    fn pip_pixels(&self) -> Option<Vec<u8>> {
        (self.show_pip.load(Ordering::Relaxed) && self.pip_fresh.load(Ordering::Relaxed))
            .then(|| self.pip_vbuf.lock().unwrap().clone())
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
    snap: Arc<tango_match::playback::Snapshot>,
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

    /// The local perspective's pixels (mgba-native BGR555). May be
    /// empty if the capture had no rendered frame.
    pub fn local_framebuffer(&self) -> &[u8] {
        &self.snap.framebuffers[self.local_player]
    }
}

/// The display surfaces an SIO playback session publishes into, plus
/// the perspective toggles that pick which core lands where — shared
/// between the drive loop, the seek worker's landing publisher, and
/// paused-frame blits so the paths can't drift (the SIO analogue of
/// [`blit_snapshot_surfaces`]).
#[derive(Clone)]
struct Surfaces {
    vbuf: Arc<Mutex<Vec<u8>>>,
    pip_vbuf: Arc<Mutex<Vec<u8>>>,
    pip_fresh: Arc<AtomicBool>,
    show_pip: Arc<AtomicBool>,
    swap_perspective: Arc<AtomicBool>,
    frame_notify: Arc<tokio::sync::Notify>,
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
            let mut vbuf = self.vbuf.lock().unwrap();
            if vbuf.len() == main.len() {
                vbuf.copy_from_slice(main);
            }
        }
        if self.show_pip.load(Ordering::Relaxed) {
            if let Some(other) = other {
                let mut pip = self.pip_vbuf.lock().unwrap();
                if pip.len() == other.len() {
                    pip.copy_from_slice(other);
                    self.pip_fresh.store(true, Ordering::Relaxed);
                }
            }
        } else {
            self.pip_fresh.store(false, Ordering::Relaxed);
        }
        self.frame_notify.notify_one();
    }

    /// Publish the pair's live framebuffers.
    fn publish_pair(&self, pair: &mut tango_match::Link) {
        let shown = self.shown();
        let main = pair.video_buffer(shown).map(|b| b.to_vec());
        let other = pair.video_buffer(1 - shown).map(|b| b.to_vec());
        self.publish(main.as_deref(), other.as_deref());
    }

    /// Publish a captured snapshot's framebuffers (emulation-free).
    fn publish_snapshot(&self, snap: &tango_match::playback::Snapshot) {
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
fn run_drive(
    boot_config: tango_match::playback::BootConfig,
    inputs: Arc<Vec<[u32; 2]>>,
    playback: SharedSioPlayback,
    cursor: Arc<AtomicU32>,
    paused: Arc<crate::PauseGate>,
    fps_bits: Arc<AtomicU32>,
    snapshots: tango_match::playback::SnapshotStore,
    rewind: tango_match::playback::RewindRing,
    cancel: Arc<AtomicBool>,
    surfaces: Surfaces,
) {
    // The display pair runs no telemetry observer — its lifecycle sink
    // is a write-only stub.
    let pb = match tango_match::playback::Playback::new(&boot_config, inputs, &tango_match::telemetry::LifecycleSink::new())
    {
        Ok(pb) => pb,
        Err(e) => {
            log::error!("sio replay: boot failed: {e:?}");
            return;
        }
    };
    *playback.lock().unwrap() = Some(pb);

    // Show the primed first frame while paused-at-start or still
    // spinning up.
    if let Some(pb) = playback.lock().unwrap().as_mut() {
        surfaces.publish_pair(pb.pair_mut());
    }

    let mut next_tick = std::time::Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        if paused.paused() {
            // Park on the gate (cancel releases it via Engine::drop);
            // restart the cadence from the wake so paused time doesn't
            // accrue pacing debt.
            paused.wait();
            next_tick = std::time::Instant::now();
            continue;
        }

        {
            let mut guard = playback.lock().unwrap();
            let Some(pb) = guard.as_mut() else { return };
            if pb.at_end() {
                paused.set(true);
                continue;
            }
            pb.step();
            cursor.store(pb.cursor(), Ordering::Relaxed);
            match pb.capture() {
                Ok(snap) => {
                    if snapshots.snapshot_needed(snap.tick) {
                        snapshots.push(snap.clone());
                    }
                    rewind.insert(snap);
                }
                Err(e) => log::warn!("sio replay: frame capture failed: {e:?}"),
            }
            surfaces.publish_pair(pb.pair_mut());
        }

        // Pace at the published target (60 × speed factor).
        let fps = f32::from_bits(fps_bits.load(Ordering::Relaxed)).max(1.0);
        next_tick += std::time::Duration::from_secs_f64(1.0 / fps as f64);
        let now = std::time::Instant::now();
        if next_tick > now {
            std::thread::sleep(next_tick - now);
        } else if now - next_tick > std::time::Duration::from_millis(250) {
            // Fell way behind (debugger, laptop lid, a long seek holding
            // the pair): resynchronize the cadence instead of sprinting.
            next_tick = now;
        }
    }
}

pub struct PrefetchStatsJob {
    pub partial_tx: futures::channel::mpsc::UnboundedSender<tango_match::analysis::MatchStats>,
    pub done: Arc<Mutex<Option<tango_match::analysis::MatchStats>>>,
    pub stats_file: std::path::PathBuf,
}
