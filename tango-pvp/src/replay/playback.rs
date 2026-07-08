//! Replay-playback engine: snapshot store, background prefetch worker
//! body, and the asynchronous seek machinery. The host owns the mgba
//! playback thread, the prefetch `std::thread`, and the seek-worker
//! `std::thread`; this module provides the work those threads do.

use std::collections::BTreeMap;
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

/// Depth of the [`RewindBuffer`]'s per-frame window behind the playhead.
/// Backward seeks within this many ticks land on an exact snapshot with
/// zero catch-up emulation, so single-frame stepping is instantaneous.
pub const REWIND_BUFFER_FRAMES: u32 = 90;

/// Hard cap on rewind-buffer entries. Must exceed `REWIND_BUFFER_FRAMES +
/// MID_ROUND_SNAPSHOT_INTERVAL + 1`: a backfill re-runs from a keyframe at
/// most an interval below the window floor, and its transient span — plus
/// whatever coverage already sits at or above the landing frame — has to
/// survive eviction or the backfill would eat its own way back. Beyond
/// that, extra entries are frames ahead of the playhead (free exact
/// forward steps); the entry farthest from the anchor goes first.
const REWIND_BUFFER_MAX_ENTRIES: usize = 192;

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

    /// Push an already-shared snapshot — used by the playback frame
    /// callback, where the [`RewindBuffer`]'s per-frame capture has
    /// already paid the `save_state` and the keyframe can share it.
    pub fn push_arc(&self, snapshot: Arc<ReplaySnapshot>) {
        self.0.lock().unwrap().push(snapshot);
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

    /// Unconditionally capture a snapshot for `cp`. Callers check
    /// [`Self::snapshot_needed`] themselves (the playback frame callback
    /// additionally shares the capture with its [`RewindBuffer`]) and
    /// pass the buffer by value because the prefetch worker can't borrow
    /// the framebuffer and the core mutably at once.
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

/// Rolling per-frame snapshot window trailing the playhead. Where the
/// [`SnapshotStore`] keeps sparse keyframes for the whole replay (one per
/// [`MID_ROUND_SNAPSHOT_INTERVAL`]), this keeps *every* frame for the last
/// [`REWIND_BUFFER_FRAMES`] ticks the playback core ran, so a short
/// backward (or re-forward) seek loads its exact snapshot instead of a
/// keyframe plus up to an interval of catch-up emulation.
///
/// The playback frame callback captures into it on every frame — normal
/// playback, seek chases, and backfill passes alike — so the window
/// follows the playhead wherever it goes. Entries are keyed by
/// recorded-frame index; a replay is deterministic, so an entry never
/// goes stale and eviction is purely a memory bound.
///
/// Evictions measure distance from the *anchor* (the playhead: captures
/// raise it as playback advances, each seek chase re-sets it to its
/// target) — never from the inserted key. A backfill inserts far below
/// the playhead, and evicting "farthest from the insert" there would
/// eat the entries *at* the playhead, making the very next step a miss
/// that re-chases the region it just evicted, over and over.
#[derive(Clone, Default)]
pub struct RewindBuffer(Arc<RewindBufferInner>);

#[derive(Default)]
struct RewindBufferInner {
    entries: Mutex<BTreeMap<u32, Arc<ReplaySnapshot>>>,
    /// The playhead tick evictions protect `[anchor - REWIND_BUFFER_FRAMES,
    /// anchor]` around.
    anchor: AtomicU32,
}

impl RewindBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Re-anchor the window at `tick`. The seek worker calls this with
    /// each chase target; normal-playback captures only ever raise it.
    pub fn set_anchor(&self, tick: u32) {
        self.0.anchor.store(tick, Ordering::Release);
    }

    /// Capture a snapshot of the current core + shadow state for `cp`,
    /// unless its frame index is already buffered (re-running a frame
    /// reproduces it exactly). Returns the entry now at that index,
    /// shared, so the caller can also push it into the keyframe store
    /// without paying a second `save_state`; None if capture failed.
    pub fn capture(
        &self,
        cp: ReplayCheckpoint,
        core: &mut mgba::core::CoreMutRef<'_>,
        shadow: &Mutex<Shadow>,
        framebuffer: &[u8],
    ) -> Option<Arc<ReplaySnapshot>> {
        let frame_index = cp.frame_index;
        // Forward playback drags the anchor along; captures below it
        // (chase catch-up, backfill) leave it where the seek put it.
        self.0.anchor.fetch_max(frame_index, Ordering::AcqRel);
        if let Some(existing) = self.0.entries.lock().unwrap().get(&frame_index) {
            return Some(existing.clone());
        }
        // Only the playback frame callback captures into a given buffer,
        // so dropping the lock across the ~400KB state saves can't race
        // in a duplicate.
        let mgba_state = core.save_state().ok()?;
        let shadow_snapshot = shadow.lock().unwrap().save_state().ok()?;
        let snap = Arc::new(ReplaySnapshot {
            checkpoint: cp,
            mgba_state,
            shadow_snapshot,
            framebuffer: framebuffer.to_vec(),
        });
        self.insert(frame_index, snap.clone());
        Some(snap)
    }

    /// Insert an externally captured snapshot — e.g. a keyframe the seek
    /// chase landed on — so it can anchor coverage walks and the
    /// backfill's landing restore. Same eviction as [`Self::capture`].
    pub fn adopt(&self, snap: Arc<ReplaySnapshot>) {
        self.insert(snap.checkpoint.frame_index, snap);
    }

    fn insert(&self, frame_index: u32, snap: Arc<ReplaySnapshot>) {
        let anchor = self.0.anchor.load(Ordering::Acquire);
        let mut entries = self.0.entries.lock().unwrap();
        entries.insert(frame_index, snap);
        // The window trails the anchor: drop everything more than a
        // window-plus-keyframe-interval behind it. The interval of slack
        // matters: a backfill runs from a keyframe up to an interval
        // below the window floor, and its captures on the way up are the
        // stepping stones the next chunk resumes from — trim at exactly
        // the floor and the fill can never make progress into the
        // window's bottom.
        let keep_from = anchor.saturating_sub(REWIND_BUFFER_FRAMES + MID_ROUND_SNAPSHOT_INTERVAL + 1);
        while let Some((&lo, _)) = entries.first_key_value() {
            if lo < keep_from {
                entries.pop_first();
            } else {
                break;
            }
        }
        // ...and bound whatever else accumulated (frames ahead of the
        // playhead, coverage left over from before a long seek) by
        // evicting the entry farthest from the anchor.
        while entries.len() > REWIND_BUFFER_MAX_ENTRIES {
            let (&lo, _) = entries.first_key_value().unwrap();
            let (&hi, _) = entries.last_key_value().unwrap();
            if lo.abs_diff(anchor) >= hi.abs_diff(anchor) {
                entries.pop_first();
            } else {
                entries.pop_last();
            }
        }
    }

    /// Largest buffered snapshot with `frame_index <= target` — same
    /// contract as [`SnapshotStore::best_at_or_before`].
    pub fn best_at_or_before(&self, target: u32) -> Option<Arc<ReplaySnapshot>> {
        self.0
            .entries
            .lock()
            .unwrap()
            .range(..=target)
            .next_back()
            .map(|(_, s)| s.clone())
    }

    /// Largest buffered snapshot with `lo_exclusive < frame_index <=
    /// hi_inclusive` — same contract as [`SnapshotStore::best_in_range`].
    pub fn best_in_range(&self, lo_exclusive: u32, hi_inclusive: u32) -> Option<Arc<ReplaySnapshot>> {
        self.0
            .entries
            .lock()
            .unwrap()
            .range((
                std::ops::Bound::Excluded(lo_exclusive),
                std::ops::Bound::Included(hi_inclusive),
            ))
            .next_back()
            .map(|(_, s)| s.clone())
    }

    /// Buffered snapshot closest to `target` on either side, if any —
    /// same contract as [`SnapshotStore::nearest`].
    pub fn nearest(&self, target: u32) -> Option<Arc<ReplaySnapshot>> {
        let entries = self.0.entries.lock().unwrap();
        let below = entries.range(..=target).next_back();
        let above = entries
            .range((std::ops::Bound::Excluded(target), std::ops::Bound::Unbounded))
            .next();
        [below, above]
            .into_iter()
            .flatten()
            .min_by_key(|(k, _)| k.abs_diff(target))
            .map(|(_, s)| s.clone())
    }

    /// The buffered snapshot exactly at `frame_index`, if any.
    fn exact(&self, frame_index: u32) -> Option<Arc<ReplaySnapshot>> {
        self.0.entries.lock().unwrap().get(&frame_index).cloned()
    }

    /// Lowest frame index reachable from `end` through contiguously
    /// buffered frames; None if `end` itself isn't buffered. The walk is
    /// bounded by the eviction window, so at most a couple hundred probes.
    fn coverage_floor(&self, end: u32) -> Option<u32> {
        let entries = self.0.entries.lock().unwrap();
        if !entries.contains_key(&end) {
            return None;
        }
        let mut floor = end;
        while floor > 0 && entries.contains_key(&(floor - 1)) {
            floor -= 1;
        }
        Some(floor)
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
        .load_save(mgba::vfile::VFile::from_vec(replay.local_sram.clone()))?;
    // Pin the cart RTC to the recorded match clock — prefetched snapshots
    // must be byte-identical to the states the playback core reaches on its
    // own, or seeking through them desyncs RTC-reading games (exe45).
    core.set_rtc_fixed(replay.rtc_time());
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
///
/// After a chase lands and the session stays paused, the worker rebuilds
/// `rewind`'s per-frame window behind the landing point in bounded
/// chunks (see [`backfill_chunk_on_core`]), so subsequent backward steps
/// hit exact snapshots. The display already shows the landing frame and
/// the publish gate stays closed for the pass, so it's invisible; a
/// newer request aborts it at the next frame boundary and the pass
/// resumes where it left off after that seek lands.
///
/// `publish_landing` displays a snapshot's stored framebuffer. Chases
/// that land by loading a snapshot exactly at the target never run a
/// frame, so the frame callback can't publish the landing — the chase
/// hands the host the pixels directly instead.
pub fn run_seek_worker(
    handle: mgba::thread::Handle,
    ctrl: Arc<SeekController>,
    stepper_state: stepper::State,
    shadow: Arc<Mutex<Shadow>>,
    replay: Arc<Replay>,
    store: SnapshotStore,
    rewind: RewindBuffer,
    completion_token: crate::hooks::CompletionToken,
    publish_landing: impl Fn(&ReplaySnapshot) + Send + Sync + 'static,
) {
    let publish_landing = Arc::new(publish_landing);
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
            let rewind = rewind.clone();
            let completion_token = completion_token.clone();
            let publish_landing = publish_landing.clone();
            handle.run_on_core(move |mut core| {
                if let Err(e) = chase_on_core(
                    core,
                    &ctrl,
                    &stepper_state,
                    &shadow,
                    &replay,
                    &store,
                    &rewind,
                    &completion_token,
                    publish_landing.as_ref(),
                ) {
                    log::error!("seek chase failed: {e:?}");
                }
                // Catch-up frames pushed fast-forward audio into the
                // core's buffer; purge it before releasing the handle or
                // the audio callback plays it as a garbled burst.
                core.audio_buffer().clear();
            });
        }
        ctrl.chasing.store(false, Ordering::Release);

        // Resume playback only once the chase has landed for good — if a
        // newer request already arrived, stay paused and let its chase
        // decide.
        if !ctrl.dirty.load(Ordering::Acquire) && ctrl.resume.swap(false, Ordering::AcqRel) {
            handle.unpause();
            // Playing forward refills the rewind window on its own; a
            // backfill here would only delay the unpause.
            continue;
        }

        // Parked after landing: rebuild the rewind window behind the
        // landing frame so backward steps keep hitting exact snapshots.
        // The pass runs in bounded chunks — one short `run_on_core`
        // slice each, with the handle released (plus a breather) in
        // between — so a racing render or transport call never waits
        // more than a chunk, and a key-repeat stepping burst refills
        // the window in the gaps between presses faster than the
        // presses drain it (each press aborts at most one chunk).
        // `chasing` stays set across the whole pass: the publish gate
        // keeps the transient rewind off the display and the UI keeps
        // drawing the playhead at the landed target.
        if !ctrl.dirty.load(Ordering::Acquire) && !ctrl.cancel.load(Ordering::Acquire) {
            ctrl.chasing.store(true, Ordering::Release);
            // A full pass is at most a window plus a keyframe interval of
            // frames (~19 chunks) plus a handful of merge/restore chunks;
            // far past that means the fill isn't converging (degenerate
            // coverage geometry) — bail to the landing rather than hold
            // the core hostage.
            let mut chunks_left = 64;
            loop {
                if ctrl.dirty.load(Ordering::Acquire) || ctrl.cancel.load(Ordering::Acquire) {
                    break;
                }
                chunks_left -= 1;
                if chunks_left <= 0 || !handle.is_paused() {
                    // Budget exhausted, or the user unpaused mid-pass:
                    // put the core back on the landing frame so playback
                    // continues from there instead of replaying the
                    // half-filled window.
                    let ctrl = ctrl.clone();
                    let stepper_state = stepper_state.clone();
                    let shadow = shadow.clone();
                    let replay = replay.clone();
                    let rewind = rewind.clone();
                    handle.run_on_core(move |mut core| {
                        if let Err(e) = restore_landing_on_core(core, &ctrl, &stepper_state, &shadow, &replay, &rewind)
                        {
                            log::error!("rewind backfill unpause restore failed: {e:?}");
                        }
                        // Drop any backfill audio before playback resumes.
                        core.audio_buffer().clear();
                    });
                    break;
                }
                let finished = Arc::new(AtomicBool::new(true));
                {
                    let ctrl = ctrl.clone();
                    let stepper_state = stepper_state.clone();
                    let shadow = shadow.clone();
                    let replay = replay.clone();
                    let store = store.clone();
                    let rewind = rewind.clone();
                    let finished = finished.clone();
                    handle.run_on_core(move |mut core| {
                        match backfill_chunk_on_core(core, &ctrl, &stepper_state, &shadow, &replay, &store, &rewind) {
                            Ok(done) => finished.store(done, Ordering::Release),
                            Err(e) => log::error!("rewind backfill failed: {e:?}"),
                        }
                        // The chunk's frames pushed audio the user must
                        // never hear — purge before the handle drops and
                        // the audio callback interleaves.
                        core.audio_buffer().clear();
                    });
                }
                if finished.load(Ordering::Acquire) {
                    break;
                }
                // Breather: hand a blocked render or transport call the
                // handle before taking it again for the next chunk.
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            ctrl.chasing.store(false, Ordering::Release);
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
    rewind: &RewindBuffer,
    completion_token: &crate::hooks::CompletionToken,
    publish_landing: &(dyn Fn(&ReplaySnapshot) + Send + Sync),
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
        // The playhead is moving to `target` — evictions now protect the
        // window behind it, wherever the core transiently roams.
        rewind.set_anchor(target);

        // The rewind buffer's per-frame window usually holds the target
        // exactly (zero catch-up); the keyframe store bounds the chase
        // at an interval everywhere else. Best = highest frame index.
        let cur = stepper_state.lock_inner().inputs_consumed();
        let start_snap = if target < cur {
            let best = [rewind.best_at_or_before(target), store.best_at_or_before(target)]
                .into_iter()
                .flatten()
                .max_by_key(|s| s.checkpoint.frame_index);
            match best {
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
            [rewind.best_in_range(cur, target), store.best_in_range(cur, target)]
                .into_iter()
                .flatten()
                .max_by_key(|s| s.checkpoint.frame_index)
        };

        if let Some(snap) = &start_snap {
            // Seed the rewind window with the starting snapshot itself:
            // the catch-up run captures every frame *after* it, and
            // without this entry coverage could never merge across a
            // keyframe start (see the same adopt in the backfill).
            rewind.adopt(snap.clone());
            core.load_state(&snap.mgba_state)?;
            stepper_state.restore_replay_checkpoint(&snap.checkpoint, &replay.rounds)?;
            // Restore shadow alongside stepper. The shadow holds its own
            // mgba core + Round state; without restoring it, post-seek
            // apply_input calls would feed the stepper packets from the
            // shadow's stale pre-seek state.
            shadow.lock().unwrap().load_state(&snap.shadow_snapshot)?;
            // A zero-frame landing (the snapshot IS the target — the
            // rewind buffer's usual case) never runs a frame, so the
            // frame callback can't put it on screen; blit the stored
            // framebuffer instead.
            if snap.checkpoint.frame_index >= target {
                publish_landing(snap);
            }
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

/// How many frames one backfill chunk may run before handing the core
/// back. Each chunk is a single `run_on_core` slice — anything blocked
/// on the mgba handle (renders reading `is_paused`/`speed`, transport
/// presses) waits at most this long. Sized to roughly one UI frame of
/// emulation + per-frame captures.
const BACKFILL_CHUNK_FRAMES: u32 = 8;

/// One bounded slice of the post-landing rewind backfill: extend the
/// per-frame window behind the landed target by up to
/// [`BACKFILL_CHUNK_FRAMES`] frames (the frame callback captures each
/// one), or — once contiguous coverage reaches the window floor — put
/// the core back on the landing frame. Returns Ok(true) when the pass
/// is finished (window full, nothing to anchor to, or superseded),
/// Ok(false) when another chunk should follow.
///
/// Chunks are stateless: the fill front is recovered from the buffer
/// itself (the highest snapshot below the covered top block), so a pass
/// aborted by a new seek resumes exactly where it left off after that
/// seek lands — this is what lets a key-repeat stepping burst refill
/// the window in the gaps between presses instead of restarting a
/// monolithic pass it never finishes.
///
/// Must be called on the thread that owns `core` — i.e. from inside an
/// mgba `run_on_core` callback.
fn backfill_chunk_on_core(
    mut core: mgba::core::CoreMutRef<'_>,
    ctrl: &SeekController,
    stepper_state: &stepper::State,
    shadow: &Mutex<Shadow>,
    replay: &Replay,
    store: &SnapshotStore,
    rewind: &RewindBuffer,
) -> anyhow::Result<bool> {
    let end = ctrl.target.load(Ordering::Acquire);

    // Anchor: the landed target's own snapshot. Normally the frame
    // callback captured it during the chase; a zero-frame landing on a
    // store keyframe adopts that keyframe instead. No anchor means the
    // chase never truly landed at `end` (dropped request, input
    // exhaustion — the chase merging ring+store would have found any
    // snapshot that exists there) — and filling behind a stale target
    // would publish frames at or past it, so don't.
    let landing = match rewind.exact(end) {
        Some(snap) => snap,
        None => {
            let keyframe = store.best_at_or_before(end).filter(|s| s.checkpoint.frame_index == end);
            match keyframe {
                Some(snap) => {
                    rewind.adopt(snap.clone());
                    snap
                }
                None => return Ok(true),
            }
        }
    };

    let lo = end.saturating_sub(REWIND_BUFFER_FRAMES);
    // The anchor guarantees coverage_floor anchors at `end`.
    let floor = rewind.coverage_floor(end).unwrap_or(end);
    let restore_landing = |core: &mut mgba::core::CoreMutRef<'_>| -> anyhow::Result<()> {
        if stepper_state.lock_inner().inputs_consumed() != end {
            core.load_state(&landing.mgba_state)?;
            stepper_state.restore_replay_checkpoint(&landing.checkpoint, &replay.rounds)?;
            shadow.lock().unwrap().load_state(&landing.shadow_snapshot)?;
        }
        Ok(())
    };
    if floor <= lo {
        // Window full — put the core back on the landing frame if an
        // earlier chunk left it mid-window.
        restore_landing(&mut core)?;
        return Ok(true);
    }

    // The fill front: the highest snapshot below the covered top block.
    // On a fresh segment that's a keyframe at (or inside) the gap;
    // afterwards it's wherever the previous chunk stopped.
    let front = [rewind.best_at_or_before(floor - 1), store.best_at_or_before(floor - 1)]
        .into_iter()
        .flatten()
        .max_by_key(|s| s.checkpoint.frame_index);
    let Some(front) = front else {
        // Nothing below the gap to run from (the window butts against
        // the start of recorded history).
        restore_landing(&mut core)?;
        return Ok(true);
    };
    // Never reach across a round boundary: the frames between rounds
    // consume no inputs, so restarting from a previous round would
    // re-run the whole inter-round animation invisibly. The window just
    // stays truncated at the round start instead.
    if front.checkpoint.current_round_index != landing.checkpoint.current_round_index {
        restore_landing(&mut core)?;
        return Ok(true);
    }

    // The front is itself a frame of the window — adopt it. A run can
    // only capture frames *after* its starting state, so a keyframe at
    // the gap's edge would otherwise never enter the buffer and
    // coverage could never merge across it (the pass would spin on it
    // forever).
    rewind.adopt(front.clone());

    // Resume from the front — the previous chunk usually left the core
    // right there, in which case the load is skipped.
    if stepper_state.lock_inner().inputs_consumed() != front.checkpoint.frame_index {
        core.load_state(&front.mgba_state)?;
        stepper_state.restore_replay_checkpoint(&front.checkpoint, &replay.rounds)?;
        shadow.lock().unwrap().load_state(&front.shadow_snapshot)?;
    }

    for _ in 0..BACKFILL_CHUNK_FRAMES {
        if ctrl.cancel.load(Ordering::Acquire) || ctrl.dirty.load(Ordering::Acquire) {
            // Superseded mid-chunk. The core is left mid-window, but the
            // next chase replans from a snapshot regardless.
            return Ok(true);
        }
        if stepper_state.lock_inner().inputs_consumed() >= floor {
            break;
        }
        core.run_frame();
    }
    Ok(false)
}

/// Put the core back on the landed target's frame if a backfill chunk
/// left it elsewhere — used when the user unpauses mid-pass, so playback
/// continues from the landing instead of replaying the half-filled
/// window. A no-op when the landing isn't buffered (then no chunk ever
/// moved the core).
fn restore_landing_on_core(
    mut core: mgba::core::CoreMutRef<'_>,
    ctrl: &SeekController,
    stepper_state: &stepper::State,
    shadow: &Mutex<Shadow>,
    replay: &Replay,
    rewind: &RewindBuffer,
) -> anyhow::Result<()> {
    let end = ctrl.target.load(Ordering::Acquire);
    let Some(landing) = rewind.exact(end) else {
        return Ok(());
    };
    if stepper_state.lock_inner().inputs_consumed() != end {
        core.load_state(&landing.mgba_state)?;
        stepper_state.restore_replay_checkpoint(&landing.checkpoint, &replay.rounds)?;
        shadow.lock().unwrap().load_state(&landing.shadow_snapshot)?;
    }
    Ok(())
}
