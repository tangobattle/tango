//! Replay playback session.
//!
//! Owns an mgba playback thread, installs the per-game stepper traps,
//! pushes captured snapshots into a [`SnapshotStore`] each frame, and
//! runs a background [`Prefetcher`] thread that races ahead of the
//! playhead to keep that store populated for seeks. Seeks are
//! asynchronous: requests land on a [`SeekController`] and a dedicated
//! [`SeekWorker`] thread chases the newest target on the mgba thread,
//! so the UI never blocks on catch-up emulation. Audio is bound via
//! the shared [`crate::audio::LateBinder`].

pub mod scrubber;

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use tango_pvp::replay::playback::{SeekController, SnapshotStore};
use tango_pvp::shadow::Shadow;

pub const SCREEN_WIDTH: u32 = mgba::gba::SCREEN_WIDTH;
pub const SCREEN_HEIGHT: u32 = mgba::gba::SCREEN_HEIGHT;
const EXPECTED_FPS: f32 = 60.0;

pub struct ReplaySession {
    game: &'static crate::game::Game,
    close_requested: Arc<AtomicBool>,
    replay: Arc<tango_pvp::replay::Replay>,
    stepper_state: tango_pvp::stepper::State,
    snapshots: SnapshotStore,
    prefetch_progress: Arc<AtomicU32>,
    total_ticks: u32,
    /// Shared display framebuffer + its wake handle, kept so
    /// [`Self::scrub_preview`] can blit snapshot framebuffers without
    /// going through the emulator at all.
    vbuf: Arc<Mutex<Vec<u8>>>,
    frame_notify: Arc<tokio::sync::Notify>,
    seek: Arc<SeekController>,
    /// Held so the audio binding survives for the session's lifetime;
    /// the LateBinder swaps back to silence when this Drops.
    _audio_binding: Option<crate::audio::Binding>,
    /// Field order matters — `_prefetcher`'s and `_seek_worker`'s Drops
    /// signal cancel and join their background threads before `thread`
    /// is torn down. All three come last so the frame callback and any
    /// in-flight seek chase (both running on `thread`) are dead by the
    /// time the earlier fields are freed.
    _prefetcher: Prefetcher,
    _seek_worker: SeekWorker,
    thread: mgba::thread::Thread,
}

impl ReplaySession {
    pub fn new(
        game: &'static crate::game::Game,
        rom: Arc<Vec<u8>>,
        remote_game: &'static crate::game::Game,
        remote_rom: Arc<Vec<u8>>,
        replay: Arc<tango_pvp::replay::Replay>,
        audio_binder: &crate::audio::LateBinder,
        frame_notify: Arc<tokio::sync::Notify>,
        vbuf: Arc<Mutex<Vec<u8>>>,
    ) -> anyhow::Result<Self> {
        let mut core = crate::session::new_gba_core(rom.as_ref())?;
        core.as_mut()
            .load_save(mgba::vfile::VFile::from_vec(replay.local_sram_dump()))?;

        let hooks = game.hooks;
        hooks.patch(core.as_mut());

        let completion_token = tango_pvp::hooks::CompletionToken::new();
        if replay.rounds.is_empty() {
            anyhow::bail!("replay has no rounds");
        }
        let replay_is_complete = replay.is_complete;
        let total_ticks = replay.rounds.iter().map(|r| r.len() as u32).sum::<u32>();

        let (stepper_state, shadow) = tango_pvp::stepper::State::new_for_replay(
            &replay,
            remote_rom.as_ref(),
            remote_game.hooks,
            Box::new({
                let completion_token = completion_token.clone();
                move || completion_token.complete()
            }),
        )?;

        let mut traps = hooks.common_traps();
        traps.extend(hooks.stepper_traps(stepper_state.clone()));
        core.set_traps(traps);

        let thread = mgba::thread::Thread::new(core);
        // Wipe the shared framebuffer so the previous session's
        // last frame doesn't flash through before mgba writes its
        // first one.
        vbuf.lock().unwrap().fill(0);

        let snapshots = SnapshotStore::new();
        let prefetch_progress = Arc::new(AtomicU32::new(0));
        let prefetcher = Prefetcher::spawn(
            rom.clone(),
            remote_rom.clone(),
            replay.clone(),
            game,
            remote_game,
            snapshots.clone(),
            prefetch_progress.clone(),
        );

        let seek = Arc::new(SeekController::new());

        thread.set_frame_callback({
            let vbuf = vbuf.clone();
            let completion_token = completion_token.clone();
            let stepper_state = stepper_state.clone();
            let snapshots = snapshots.clone();
            let shadow = shadow.clone();
            let frame_notify = frame_notify.clone();
            let seek = seek.clone();
            move |mut core, video_buffer, mut thread_handle| {
                let (absolute_tick, total_left, is_round_ended) = {
                    let mut inner = stepper_state.lock_inner();
                    if let Some(err) = inner.take_error() {
                        log::error!("replay stepper error: {err:?}");
                    }
                    (
                        inner.absolute_tick(),
                        inner.total_input_pairs_left(),
                        inner.is_round_ended(),
                    )
                };

                // During a seek chase only the landing frame reaches the
                // display; publishing every intermediate catch-up frame
                // would strobe a fast-forward of everything in between.
                if seek.should_publish_frame(absolute_tick) {
                    // Copy mgba's native BGR555 straight through; the framebuffer
                    // shader expands it to RGB on the GPU at draw time.
                    vbuf.lock().unwrap().copy_from_slice(video_buffer);
                    // the texture handle for this frame. See
                    // `singleplayer_session` for rationale.
                    frame_notify.notify_one();
                }

                // Capture round-start + every MID_ROUND_SNAPSHOT_INTERVAL
                // ticks so backward seeks have a nearby jumping-off point
                // even if the prefetcher hasn't reached them yet.
                if let Some(cp) = stepper_state.capture_replay_checkpoint() {
                    snapshots.capture_if_needed(cp, &mut core, &shadow, video_buffer);
                }

                // Mirrors the legacy guard: clean replays wait for the
                // post-round end-of-round routine to flip is_round_ended;
                // incomplete replays just fall through on input exhaustion.
                if total_left == 0 && (is_round_ended || !replay_is_complete) {
                    completion_token.complete();
                }

                if completion_token.is_complete() {
                    thread_handle.pause();
                }
            }
        });

        thread.start()?;
        thread.handle().lock_audio().sync_mut().set_fps_target(EXPECTED_FPS);

        let seek_worker = SeekWorker::spawn(
            thread.handle(),
            seek.clone(),
            stepper_state.clone(),
            shadow.clone(),
            replay.clone(),
            snapshots.clone(),
            completion_token.clone(),
        );

        let audio_binding = audio_binder.bind_mgba(thread.handle(), "replay");

        Ok(Self {
            game,
            close_requested: Arc::new(AtomicBool::new(false)),
            replay,
            stepper_state,
            snapshots,
            prefetch_progress,
            total_ticks,
            vbuf,
            frame_notify,
            seek,
            _audio_binding: audio_binding,
            _prefetcher: prefetcher,
            _seek_worker: seek_worker,
            thread,
        })
    }

    pub fn game(&self) -> &'static crate::game::Game {
        self.game
    }

    pub fn is_paused(&self) -> bool {
        self.thread.handle().is_paused()
    }

    /// Drive the mgba thread at `factor * 60` fps. 1.0 = realtime,
    /// 0.5 = slow-mo, 2.0 / 4.0 = fast-forward. Audio paces frames, so
    /// values above ~4 start dropping samples.
    pub fn set_speed(&self, factor: f32) {
        let fps = (EXPECTED_FPS * factor).max(1.0);
        self.thread.handle().lock_audio().sync_mut().set_fps_target(fps);
    }

    /// Current factor (current fps / 60).
    pub fn speed(&self) -> f32 {
        self.thread.handle().lock_audio().sync().fps_target() / EXPECTED_FPS
    }

    /// Toggle the mgba thread between paused and running. Returns the
    /// new paused state.
    pub fn set_paused(&self, paused: bool) {
        let h = self.thread.handle();
        if paused {
            h.pause();
        } else {
            // The frame_callback re-pauses when completion_token is set,
            // so unpausing past the end of a replay only buys one frame
            // — that's fine and matches the legacy behavior.
            h.unpause();
        }
    }

    pub fn request_close(&self) {
        self.close_requested.store(true, Ordering::Release);
    }

    /// Absolute playhead tick: how many emulated ticks the stepper has
    /// consumed since the start of the replay. Saturates at
    /// `total_ticks` once the replay finishes.
    pub fn current_tick(&self) -> u32 {
        self.stepper_state.lock_inner().absolute_tick()
    }

    pub fn total_ticks(&self) -> u32 {
        self.total_ticks
    }

    /// Highest tick the background prefetcher has reached, for the
    /// progress overlay on the scrub bar. Hits `total_ticks` when the
    /// prefetcher has run to completion.
    pub fn prefetch_progress(&self) -> u32 {
        self.prefetch_progress.load(Ordering::Relaxed)
    }

    /// Cumulative tick at the end of each round *except* the last —
    /// the inter-round boundaries the scrubber draws marks at. Empty
    /// for a single-round replay.
    pub fn round_boundaries(&self) -> Vec<u32> {
        let n = self.replay.rounds.len();
        if n <= 1 {
            return Vec::new();
        }
        let mut acc = 0u32;
        let mut out = Vec::with_capacity(n - 1);
        for r in self.replay.rounds.iter().take(n - 1) {
            acc += r.len() as u32;
            out.push(acc);
        }
        out
    }

    /// Jump the playhead to `target`, asynchronously. Records the request
    /// on the seek controller and returns immediately; the seek worker
    /// runs the snapshot load + frame catch-up on the mgba thread, and
    /// newer requests supersede in-flight ones mid-chase. With
    /// `resume_after`, playback unpauses once the chase lands (unless a
    /// newer request took over) — used by scrub commits, which pause
    /// playback for the duration of the drag.
    pub fn seek_to(&self, target: u32, resume_after: bool) {
        self.seek.request(target.min(self.total_ticks), resume_after);
    }

    /// Target of the in-flight seek, if any — lets the UI draw the
    /// playhead where it's headed instead of snapping back to the
    /// pre-seek tick until the chase lands.
    pub fn pending_seek_target(&self) -> Option<u32> {
        self.seek.pending_target()
    }

    /// True while an in-flight seek will unpause playback on landing.
    /// The thread is paused for the chase's duration, but the session
    /// is logically still playing — the transport shouldn't flip to
    /// the paused state.
    pub fn seek_will_resume(&self) -> bool {
        self.seek.resume_pending()
    }

    /// Withdraw an in-flight seek's pending resume, keeping playback
    /// paused once it lands.
    pub fn cancel_seek_resume(&self) {
        self.seek.clear_resume();
    }

    /// The captured snapshot nearest `target`, if any — backs the hover
    /// thumbnail above the scrub bar. The snapshot's framebuffer is
    /// mgba-native BGR555, same as the shared display buffer.
    pub fn nearest_snapshot(&self, target: u32) -> Option<std::sync::Arc<tango_pvp::stepper::ReplaySnapshot>> {
        self.snapshots.nearest(target)
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
        let Some(snap) = self.snapshots.nearest(target) else {
            return false;
        };
        if !force_keyframe {
            let cur = self.stepper_state.lock_inner().absolute_tick();
            if cur.abs_diff(target) <= snap.checkpoint.absolute_tick.abs_diff(target) {
                return false;
            }
        }
        {
            let mut vbuf = self.vbuf.lock().unwrap();
            if vbuf.len() != snap.framebuffer.len() {
                return false;
            }
            vbuf.copy_from_slice(&snap.framebuffer);
        }
        self.frame_notify.notify_one();
        true
    }
}

/// Owns the seek worker thread driving [`SeekController`] requests
/// against the playback core. The worker — not the requester — eats the
/// blocking `run_on_core` round-trip per chase.
///
/// Drop cancels the controller (aborting any in-flight chase at its
/// next frame boundary) and joins.
struct SeekWorker {
    ctrl: Arc<SeekController>,
    join_handle: Option<std::thread::JoinHandle<()>>,
}

impl SeekWorker {
    fn spawn(
        handle: mgba::thread::Handle,
        ctrl: Arc<SeekController>,
        stepper_state: tango_pvp::stepper::State,
        shadow: Arc<Mutex<Shadow>>,
        replay: Arc<tango_pvp::replay::Replay>,
        snapshots: SnapshotStore,
        completion_token: tango_pvp::hooks::CompletionToken,
    ) -> Self {
        let join_handle = std::thread::spawn({
            let ctrl = ctrl.clone();
            move || {
                tango_pvp::replay::playback::run_seek_worker(
                    handle,
                    ctrl,
                    stepper_state,
                    shadow,
                    replay,
                    snapshots,
                    completion_token,
                );
            }
        });
        Self {
            ctrl,
            join_handle: Some(join_handle),
        }
    }
}

impl Drop for SeekWorker {
    fn drop(&mut self) {
        self.ctrl.shutdown();
        if let Some(h) = self.join_handle.take() {
            let _ = h.join();
        }
    }
}

/// Background snapshot-prefetch worker. Spawns a fresh mgba core +
/// shadow on a std::thread and races ahead of the playhead, pushing
/// captured snapshots into the shared [`SnapshotStore`] so backward
/// (and long-forward) seeks have a nearby jumping-off point.
///
/// Drop cancels the worker and joins.
pub struct Prefetcher {
    cancel: Arc<AtomicBool>,
    join_handle: Option<std::thread::JoinHandle<()>>,
}

impl Prefetcher {
    pub fn spawn(
        rom: Arc<Vec<u8>>,
        remote_rom: Arc<Vec<u8>>,
        replay: Arc<tango_pvp::replay::Replay>,
        game: &'static crate::game::Game,
        remote_game: &'static crate::game::Game,
        snapshots: SnapshotStore,
        progress: Arc<AtomicU32>,
    ) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_for_thread = cancel.clone();
        let hooks = game.hooks;
        let remote_hooks = remote_game.hooks;
        let join_handle = std::thread::spawn(move || {
            if let Err(e) = tango_pvp::replay::playback::run_prefetch(
                rom.as_ref(),
                remote_rom.as_ref(),
                &replay,
                hooks,
                remote_hooks,
                snapshots,
                cancel_for_thread,
                progress,
            ) {
                log::error!("replay prefetch worker exited with error: {e:?}");
            }
        });
        Self {
            cancel,
            join_handle: Some(join_handle),
        }
    }
}

impl Drop for Prefetcher {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(h) = self.join_handle.take() {
            let _ = h.join();
        }
    }
}
