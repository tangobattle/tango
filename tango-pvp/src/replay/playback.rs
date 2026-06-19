//! Replay-playback engine: snapshot store, background prefetch worker
//! body, and the asynchronous seek machinery. The host owns the mgba
//! playback thread, the prefetch `std::thread`, and the seek-worker
//! `std::thread`; this module provides the work those threads do.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use crate::hooks::Hooks;
use crate::replay::Replay;
use crate::shadow::Shadow;
use crate::stepper::{self, ReplayCheckpoint, ReplaySnapshot};

/// Take a fresh mid-round snapshot at most once per this many absolute_ticks.
/// Coarser snapshots leave long fast-forwards on backward scrub; finer ones
/// cost RAM and a `core.save_state()` per capture (~256KB each). 60 ticks
/// ≈ 1 seconds of GBA time.
pub const MID_ROUND_SNAPSHOT_INTERVAL: u32 = 60;

/// Shared, cloneable handle to the replay-playback snapshot collection.
/// All clones share the same underlying snapshot list behind a mutex —
/// the prefetch worker, the mgba frame callback, and the seek chase all
/// push into the same store. Snapshots are handed out as `Arc`s so a
/// seek or preview never copies the ~0.5MB state pair.
#[derive(Clone, Default)]
pub struct SnapshotStore(Arc<Mutex<Vec<Arc<ReplaySnapshot>>>>);

impl SnapshotStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// True if pushing a snapshot for `cp` would fill a gap — either no
    /// round-start snapshot exists yet for this round, or no mid-round
    /// snapshot has been taken within the prior `MID_ROUND_SNAPSHOT_INTERVAL`.
    pub fn snapshot_needed(&self, cp: &ReplayCheckpoint) -> bool {
        let snaps = self.0.lock().unwrap();
        let want_round_start = !cp.has_committed_this_round
            && !snaps.iter().any(|s| {
                s.checkpoint.current_round_index == cp.current_round_index && !s.checkpoint.has_committed_this_round
            });
        let lo = cp.frame_index.saturating_sub(MID_ROUND_SNAPSHOT_INTERVAL);
        let want_mid_round = cp.has_committed_this_round
            && !snaps
                .iter()
                .any(|s| s.checkpoint.frame_index > lo && s.checkpoint.frame_index <= cp.frame_index);
        want_round_start || want_mid_round
    }

    pub fn push(&self, snapshot: ReplaySnapshot) {
        self.0.lock().unwrap().push(Arc::new(snapshot));
    }

    /// Largest snapshot with `frame_index <= target`, if any. Used by
    /// backward seek to pick the closest jumping-off point. `target` is a
    /// recorded-frame index (see [`stepper::InnerState::inputs_consumed`]).
    pub fn best_at_or_before(&self, target: u32) -> Option<Arc<ReplaySnapshot>> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.checkpoint.frame_index <= target)
            .max_by_key(|s| s.checkpoint.frame_index)
            .cloned()
    }

    /// Largest snapshot with `lo_exclusive < frame_index <= hi_inclusive`,
    /// if any. Used by forward seek to skip frames when a prefetched
    /// snapshot lands closer to the target than the current playhead.
    pub fn best_in_range(&self, lo_exclusive: u32, hi_inclusive: u32) -> Option<Arc<ReplaySnapshot>> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.checkpoint.frame_index > lo_exclusive && s.checkpoint.frame_index <= hi_inclusive)
            .max_by_key(|s| s.checkpoint.frame_index)
            .cloned()
    }

    /// Snapshot whose frame index is closest to `target` on either side, if
    /// any. Used for the drag preview, where showing the frame just *after*
    /// the cursor beats showing one a full interval before it.
    pub fn nearest(&self, target: u32) -> Option<Arc<ReplaySnapshot>> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .min_by_key(|s| s.checkpoint.frame_index.abs_diff(target))
            .cloned()
    }

    /// Capture a snapshot for `cp` if the store has a gap there. Checks the
    /// store under the lock first so `core.save_state` / `shadow.save_state`
    /// (both ~256KB allocations) only fire on a hit.
    pub fn capture_if_needed(
        &self,
        cp: ReplayCheckpoint,
        core: &mut mgba::core::CoreMutRef<'_>,
        shadow: &Mutex<Shadow>,
        framebuffer: &[u8],
    ) {
        if !self.snapshot_needed(&cp) {
            return;
        }
        self.capture(cp, core, shadow, framebuffer.to_vec());
    }

    /// Unconditionally capture a snapshot for `cp`. Callers that can't
    /// borrow the framebuffer and the core mutably at once (the prefetch
    /// worker owns its `Core`) check [`Self::snapshot_needed`] themselves
    /// and pass the buffer by value.
    pub fn capture(
        &self,
        cp: ReplayCheckpoint,
        core: &mut mgba::core::CoreMutRef<'_>,
        shadow: &Mutex<Shadow>,
        framebuffer: Vec<u8>,
    ) {
        let Ok(state) = core.save_state() else {
            return;
        };
        let Ok(shadow_snapshot) = shadow.lock().unwrap().save_state() else {
            return;
        };
        self.push(ReplaySnapshot {
            checkpoint: cp,
            mgba_state: state,
            shadow_snapshot,
            framebuffer,
        });
    }
}

/// Body of the background prefetch worker. Spins up a fresh mgba core +
/// shadow and runs forward against `replay` as fast as the host CPU
/// allows, pushing snapshots into `store` and writing the latest
/// `absolute_tick` to `progress` after every frame. Returns `Ok(())` when
/// the replay is exhausted or `cancel` flips true.
///
/// The host spawns the thread that drives this — this function takes no
/// thread handle and never blocks except on its own work loop.
pub fn run_prefetch(
    local_rom: &[u8],
    remote_rom: &[u8],
    replay: &Replay,
    local_hooks: &'static (dyn Hooks + Send + Sync),
    remote_hooks: &'static (dyn Hooks + Send + Sync),
    store: SnapshotStore,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU32>,
) -> anyhow::Result<()> {
    let mut core = mgba::core::Core::new_gba("tango-prefetch", &mgba::core::Options { ..Default::default() })?;
    core.enable_video_buffer();
    core.as_mut()
        .load_rom(mgba::vfile::VFile::from_vec(local_rom.to_vec()))?;
    core.as_mut()
        .load_save(mgba::vfile::VFile::from_vec(replay.local_sram_dump()))?;
    // mgba::thread::Thread::start does this implicitly for the playback
    // core; a raw Core driven by run_frame needs it explicitly.
    core.as_mut().reset();

    let (stepper_state, shadow) = stepper::State::new_for_replay(replay, remote_rom, remote_hooks, Box::new(|| {}))?;
    local_hooks.install_on_stepper(&mut core, stepper_state.clone());

    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }

        let (total_left, inputs_consumed, total_replay_ticks) = {
            let inner = stepper_state.lock_inner();
            (
                inner.total_input_pairs_left(),
                inner.inputs_consumed(),
                inner.total_replay_ticks(),
            )
        };
        if total_left == 0 && inputs_consumed > 0 {
            // Reached end-of-replay. Mark the bar fully buffered and exit —
            // the playback thread will pick up from existing snapshots from
            // here on out.
            progress.store(total_replay_ticks, Ordering::Relaxed);
            return Ok(());
        }

        if let Some(cp) = stepper_state.capture_replay_checkpoint() {
            if store.snapshot_needed(&cp) {
                let framebuffer = core.video_buffer().map(|b| b.to_vec()).unwrap_or_default();
                let mut core_ref = core.as_mut();
                store.capture(cp, &mut core_ref, &shadow, framebuffer);
            }
        }

        progress.store(inputs_consumed, Ordering::Relaxed);
        core.as_mut().run_frame();
    }
}

/// Coordination state between seek requesters (the UI thread), the seek
/// worker thread, and the playback core's frame callback. Requests
/// coalesce: only the most recent target matters, and an in-flight chase
/// retargets mid-loop instead of finishing stale work.
pub struct SeekController {
    /// Latest requested absolute tick.
    target: AtomicU32,
    /// `target` holds a request no chase has consumed yet.
    dirty: AtomicBool,
    /// A chase is currently running on the playback core.
    chasing: AtomicBool,
    /// Unpause the playback thread once the chase lands (set by seeks
    /// that paused playback for the duration, e.g. a scrub drag).
    resume: AtomicBool,
    /// Tells the worker and any in-flight chase to exit.
    cancel: AtomicBool,
    wake_mutex: Mutex<()>,
    wake_cv: Condvar,
}

impl Default for SeekController {
    fn default() -> Self {
        Self::new()
    }
}

impl SeekController {
    pub fn new() -> Self {
        Self {
            target: AtomicU32::new(0),
            dirty: AtomicBool::new(false),
            chasing: AtomicBool::new(false),
            resume: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
            wake_mutex: Mutex::new(()),
            wake_cv: Condvar::new(),
        }
    }

    /// Record `target` as the newest seek request and wake the worker.
    /// Supersedes any not-yet-landed request. Never blocks on the core.
    pub fn request(&self, target: u32, resume_after: bool) {
        self.target.store(target, Ordering::Release);
        self.resume.store(resume_after, Ordering::Release);
        self.dirty.store(true, Ordering::Release);
        // Hold the wake mutex across notify so the signal can't slip
        // between the worker's dirty check and its wait.
        let _guard = self.wake_mutex.lock().unwrap();
        self.wake_cv.notify_one();
    }

    /// Permanently stop the worker (and abort any in-flight chase).
    pub fn shutdown(&self) {
        self.cancel.store(true, Ordering::Release);
        let _guard = self.wake_mutex.lock().unwrap();
        self.wake_cv.notify_one();
    }

    /// Target of the not-yet-landed seek, if any. Lets the UI draw the
    /// playhead where it's headed instead of where the core still is.
    pub fn pending_target(&self) -> Option<u32> {
        (self.dirty.load(Ordering::Acquire) || self.chasing.load(Ordering::Acquire))
            .then(|| self.target.load(Ordering::Acquire))
    }

    /// True while a not-yet-landed seek will unpause playback when it
    /// lands. The playback thread is technically paused during the
    /// chase, but showing that to the user reads as "paused" when the
    /// session is really just mid-seek — the UI should keep displaying
    /// the playing state.
    pub fn resume_pending(&self) -> bool {
        (self.dirty.load(Ordering::Acquire) || self.chasing.load(Ordering::Acquire))
            && self.resume.load(Ordering::Acquire)
    }

    /// Withdraw a pending resume: the seek still lands, but playback
    /// stays paused afterwards. Lets a pause press during the chase win
    /// over the resume the commit scheduled.
    pub fn clear_resume(&self) {
        self.resume.store(false, Ordering::Release);
    }

    /// Whether the frame at `frame_index` should reach the display.
    /// During a chase only the landing frame passes — publishing every
    /// intermediate catch-up frame strobes a fast-forward of everything
    /// between the start snapshot and the target. `frame_index` is the
    /// recorded-frame index, same scale as the target.
    pub fn should_publish_frame(&self, frame_index: u32) -> bool {
        !self.chasing.load(Ordering::Acquire) || frame_index >= self.target.load(Ordering::Acquire)
    }
}

/// Body of the seek worker thread. Sleeps until [`SeekController::request`]
/// wakes it, then runs a chase on the playback core via `run_on_core`
/// (which blocks this worker — not the requester — until the chase
/// finishes). Loops until [`SeekController::shutdown`].
pub fn run_seek_worker(
    handle: mgba::thread::Handle,
    ctrl: Arc<SeekController>,
    stepper_state: stepper::State,
    shadow: Arc<Mutex<Shadow>>,
    replay: Arc<Replay>,
    store: SnapshotStore,
    completion_token: crate::hooks::CompletionToken,
) {
    loop {
        {
            let mut guard = ctrl.wake_mutex.lock().unwrap();
            loop {
                if ctrl.cancel.load(Ordering::Acquire) {
                    return;
                }
                if ctrl.dirty.load(Ordering::Acquire) {
                    break;
                }
                guard = ctrl.wake_cv.wait(guard).unwrap();
            }
        }

        ctrl.chasing.store(true, Ordering::Release);
        {
            let ctrl = ctrl.clone();
            let stepper_state = stepper_state.clone();
            let shadow = shadow.clone();
            let replay = replay.clone();
            let store = store.clone();
            let completion_token = completion_token.clone();
            handle.run_on_core(move |core| {
                if let Err(e) = chase_on_core(core, &ctrl, &stepper_state, &shadow, &replay, &store, &completion_token)
                {
                    log::error!("seek chase failed: {e:?}");
                }
            });
        }
        ctrl.chasing.store(false, Ordering::Release);

        // Resume playback only once the chase has landed for good — if a
        // newer request already arrived, stay paused and let its chase
        // decide.
        if !ctrl.dirty.load(Ordering::Acquire) && ctrl.resume.swap(false, Ordering::AcqRel) {
            handle.unpause();
        }
    }
}

/// Catch-up loop on the playback mgba core: loads the best starting
/// snapshot for the controller's current target, then runs frames until
/// the stepper reaches it — re-planning from scratch whenever a newer
/// request lands mid-chase, so stale targets are abandoned rather than
/// chased to completion.
///
/// Must be called on the thread that owns `core` — i.e. from inside an
/// mgba `run_on_core` callback.
fn chase_on_core(
    mut core: mgba::core::CoreMutRef<'_>,
    ctrl: &SeekController,
    stepper_state: &stepper::State,
    shadow: &Mutex<Shadow>,
    replay: &Replay,
    store: &SnapshotStore,
    completion_token: &crate::hooks::CompletionToken,
) -> anyhow::Result<()> {
    // Clear completion so the frame callback's pause-on-complete check
    // doesn't immediately re-pause after a seek backwards from the end
    // of the replay.
    completion_token.reset();

    'plan: loop {
        // Order matters: clear dirty before reading the target so a
        // request racing in between is either picked up now or re-flags
        // dirty for the next pass.
        ctrl.dirty.store(false, Ordering::Release);
        let target = ctrl.target.load(Ordering::Acquire);

        let cur = stepper_state.lock_inner().inputs_consumed();
        let start_snap = if target < cur {
            match store.best_at_or_before(target) {
                Some(snap) => Some(snap),
                None => {
                    // Pre-first-round boot window — no snapshot to land
                    // on. Silently drop instead of bubbling up an error;
                    // the user can drag the scrubber further right.
                    log::debug!("seek: no snapshot at or before tick {target}");
                    if ctrl.dirty.load(Ordering::Acquire) {
                        continue 'plan;
                    }
                    return Ok(());
                }
            }
        } else {
            store.best_in_range(cur, target)
        };

        if let Some(snap) = &start_snap {
            core.load_state(&snap.mgba_state)?;
            stepper_state.restore_replay_checkpoint(&snap.checkpoint, &replay.rounds)?;
            // Restore shadow alongside stepper. The shadow holds its own
            // mgba core + Round state; without restoring it, post-seek
            // apply_input calls would feed the stepper packets from the
            // shadow's stale pre-seek state.
            shadow.lock().unwrap().load_state(&snap.shadow_snapshot)?;
        }

        loop {
            if ctrl.cancel.load(Ordering::Acquire) {
                return Ok(());
            }
            if ctrl.dirty.load(Ordering::Acquire) {
                continue 'plan;
            }
            if stepper_state.lock_inner().inputs_consumed() >= target {
                return Ok(());
            }
            if completion_token.is_complete() {
                // Inputs ran dry before reaching `target` (incomplete
                // replay) — running further frames can't advance the tick.
                return Ok(());
            }
            core.run_frame();
        }
    }
}
