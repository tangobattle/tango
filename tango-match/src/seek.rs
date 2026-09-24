//! Requesting a seek, and being told where one got to.
//!
//! This is pure orchestration — atomics and a condvar — with no
//! emulator anywhere in it, which is why it sits in the seam
//! rather than in a backend. A host's UI thread posts targets, a worker
//! chases the newest one on whatever engine is behind the replay, and
//! the two never learn anything about each other.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Condvar, Mutex};

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
