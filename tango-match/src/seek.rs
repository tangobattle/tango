//! Requesting a seek, being told where one got to, and the chase that
//! gets it there.
//!
//! The [`SeekController`] is pure orchestration — atomics and a condvar
//! — so a host's UI thread posts targets, a worker chases the newest one
//! on whatever engine is behind the replay, and the two never learn
//! anything about each other. The chase walks the engine-independent
//! [`Playback`] seam between its captures, which is why none of it sits
//! in a backend.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use crate::replay::{Capture, Playback, RewindRing, SnapshotStore};

/// Coordination state between seek requesters (the UI thread) and the
/// seek worker chasing on the playback pair. Requests
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

    // --- worker-side surface, for the seek workers outside this module
    // (the hosts driving [`crate::Replay`]'s chase).

    /// Block until a request lands ([`Self::request`]) or the controller
    /// shuts down. Returns false on shutdown.
    pub fn wait_for_request(&self) -> bool {
        let mut guard = self.wake_mutex.lock().unwrap();
        loop {
            if self.cancel.load(Ordering::Acquire) {
                return false;
            }
            if self.dirty.load(Ordering::Acquire) {
                return true;
            }
            guard = self.wake_cv.wait(guard).unwrap();
        }
    }

    /// Mark a chase pass running — [`Self::pending_target`] keeps
    /// reporting until [`Self::end_pass`].
    pub fn begin_pass(&self) {
        self.chasing.store(true, Ordering::Release);
    }

    pub fn end_pass(&self) {
        self.chasing.store(false, Ordering::Release);
    }

    /// Consume the pending request: clears dirty and returns the target.
    /// Order matters — dirty clears before the read, so a request racing
    /// in re-flags for the next pass instead of being lost.
    pub fn take_target(&self) -> u32 {
        self.dirty.store(false, Ordering::Release);
        self.target.load(Ordering::Acquire)
    }

    /// A newer request landed mid-pass — abandon the current chase.
    pub fn is_dirty(&self) -> bool {
        self.dirty.load(Ordering::Acquire)
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }

    /// Consume a pending resume-on-landing, if one was requested.
    pub fn take_resume(&self) -> bool {
        self.resume.swap(false, Ordering::AcqRel)
    }
}

/// What one slice of a seek did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeekStep {
    /// No request was pending; nothing happened.
    Idle,
    /// Still walking — call again.
    Working,
    /// The chase landed (or gave up on a plan it couldn't make).
    Landed,
}

/// One seek chase, walked a slice of ticks at a time.
///
/// A chase is a plan (find the best capture at or before the target and
/// load it) followed by a walk (step to the target, capturing as it
/// goes). Splitting it this way is what lets a host without threads run
/// one: each step does a bounded amount of work and returns, so a
/// browser can keep painting and a desktop can keep its dedicated
/// worker thread.
///
/// A newer request landing mid-walk re-plans on the next step, and a
/// cancelled controller ends the pass wherever it is.
#[derive(Default)]
pub(crate) struct SeekChase {
    /// Where this chase is going, once it has planned a route. `None`
    /// between chases and after a re-plan.
    target: Option<u32>,
    /// Newest capture taken on the way, published on landing.
    landing: Option<Arc<Capture>>,
}

enum Plan {
    /// Walk to this target.
    Walk(u32),
    /// Nothing left to do — landed on the plan, or couldn't make one.
    Done,
}

impl SeekChase {
    /// Advance a seek by at most `budget` ticks, planning one first if a
    /// request is pending.
    ///
    /// `on_progress` reports the moving cursor, `publish_landing` shows
    /// the landing capture, and `on_resume` unpauses a host whose seek
    /// asked to resume playback when it lands.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn step(
        &mut self,
        ctrl: &SeekController,
        playback: &Mutex<Playback>,
        store: &SnapshotStore,
        rewind: &RewindRing,
        budget: u32,
        on_progress: &mut dyn FnMut(u32),
        publish_landing: &mut dyn FnMut(&Capture),
        on_resume: &mut dyn FnMut(),
    ) -> SeekStep {
        if ctrl.is_cancelled() {
            return self.finish(ctrl, on_resume);
        }
        if self.target.is_none() && !ctrl.is_dirty() {
            return SeekStep::Idle;
        }
        // One lock for the whole slice: with an unbounded budget (a
        // worker thread) that means the pair is held for the entire
        // chase — nothing else may step it out from under a walk.
        let mut guard = playback.lock().unwrap();
        let pb = &mut *guard;
        if self.target.is_none() {
            match self.plan(ctrl, pb, store, rewind, on_progress, publish_landing) {
                Plan::Walk(target) => self.target = Some(target),
                Plan::Done => {
                    drop(guard);
                    return self.finish(ctrl, on_resume);
                }
            }
        }
        let target = self.target.expect("planned above");
        for _ in 0..budget {
            if pb.cursor() >= target {
                break;
            }
            if ctrl.is_cancelled() {
                drop(guard);
                return self.finish(ctrl, on_resume);
            }
            if ctrl.is_dirty() {
                // A newer target: abandon this walk and re-plan on the
                // next step, with the pass still open.
                self.target = None;
                self.landing = None;
                drop(guard);
                return SeekStep::Working;
            }
            if !pb.step_muted() {
                break;
            }
            on_progress(pb.cursor());
            match pb.capture() {
                Ok(snap) => {
                    if store.snapshot_needed(snap.tick()) {
                        store.push(snap.tick(), snap.clone());
                    }
                    rewind.insert(snap.tick(), snap.clone());
                    self.landing = Some(snap);
                }
                Err(e) => log::warn!("replay seek: capture failed: {e:?}"),
            }
        }
        if pb.cursor() < target && pb.cursor() < pb.total() {
            drop(guard);
            return SeekStep::Working;
        }
        // The walk discarded its sound per tick; this catches what a
        // walk never stepped over — the pre-seek tail a zero-length
        // chase leaves queued, which belongs to the position being left.
        pb.discard_audio();
        drop(guard);
        if let Some(snap) = self.landing.take() {
            publish_landing(&snap);
        }
        self.finish(ctrl, on_resume)
    }

    /// Plan a chase: consume the request, load the best capture at or
    /// before it, and say whether there's a walk left to do.
    fn plan(
        &mut self,
        ctrl: &SeekController,
        pb: &mut Playback,
        store: &SnapshotStore,
        rewind: &RewindRing,
        on_progress: &mut dyn FnMut(u32),
        publish_landing: &mut dyn FnMut(&Capture),
    ) -> Plan {
        ctrl.begin_pass();
        let target = ctrl.take_target();
        rewind.set_anchor(target);

        let cur = pb.cursor();
        let start = if target < cur {
            let best = [rewind.best_at_or_before(target), store.best_at_or_before(target)]
                .into_iter()
                .flatten()
                .max_by_key(|s| s.tick());
            match best {
                Some(snap) => Some(snap),
                None => return Plan::Done,
            }
        } else {
            [
                rewind.best_in_range(cur, target.max(cur)),
                store.best_in_range(cur, target.max(cur)),
            ]
            .into_iter()
            .flatten()
            .max_by_key(|s| s.tick())
        };

        if let Some(snap) = &start {
            rewind.insert(snap.tick(), snap.clone());
            if let Err(e) = pb.load(snap) {
                log::error!("replay seek: capture load failed: {e:?}");
                return Plan::Done;
            }
            on_progress(pb.cursor());
            if snap.tick() >= target {
                publish_landing(snap);
                return Plan::Done;
            }
        }
        Plan::Walk(target)
    }

    /// End the pass: clear the chase, and run the resume the seek
    /// scheduled unless a newer request has already superseded it.
    fn finish(&mut self, ctrl: &SeekController, on_resume: &mut dyn FnMut()) -> SeekStep {
        self.target = None;
        self.landing = None;
        ctrl.end_pass();
        if !ctrl.is_dirty() && !ctrl.is_cancelled() && ctrl.take_resume() {
            on_resume();
        }
        SeekStep::Landed
    }
}
