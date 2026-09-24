//! The three loops a replay session needs run — playing, seeking, and
//! the prefetch pass — as workers a host gives threads to or
//! interleaves on one event loop.

use super::*;

/// Everything the playback half of a replay session needs, whoever is
/// driving it. A desktop gives each of the session's three concerns a
/// thread of its own (see [`Workers::split`]); a browser has one event
/// loop, so [`Driver`] interleaves them.
pub(super) struct Playhead {
    pub(super) set: Arc<tango_match::ReplaySet>,
    pub(super) playback: SharedPlayback,
    pub(super) cursor: Arc<AtomicU32>,
    pub(super) paused: Arc<crate::PauseGate>,
    pub(super) cancel: Arc<AtomicBool>,
    pub(super) perspective: Perspective,
    /// The producing end of the ring the host's stream is already bound
    /// to, handed to the pair when the boot brings one up. `None` once
    /// it has been; until then the ring reads empty and the stream
    /// primes.
    pub(super) audio: Mutex<Option<tango_match::AudioIn>>,
    pub(super) seat: Arc<AtomicUsize>,
    /// Raised when the boot below is over — landed or failed — for the
    /// host's priming notice (see [`Engine::booted`]).
    pub(super) booted: Arc<AtomicBool>,
    /// Why the boot failed, if it did (see [`Engine::prime_error`]).
    pub(super) prime_error: Arc<Mutex<Option<tango_match::Error>>>,
    pub(super) speed: Arc<SpeedControl>,
}

impl Playhead {
    /// Boot + prime the display pair and show its first frame. Blocks
    /// for the priming walk — the one part of a session that can't be
    /// sliced, since it runs until the games' own traps say it's there.
    fn boot(&self) -> bool {
        let mut pb = match self.set.playback() {
            Ok(pb) => pb,
            // Torn down mid-prime — the host is waiting on this thread's
            // join, not on a session that will never come up. Nothing to
            // report: the session it would report to is going away.
            Err(tango_match::Error::Cancelled) => return false,
            Err(e) => {
                log::error!("replay: boot failed: {e:?}");
                *self.prime_error.lock().unwrap() = Some(e);
                // The view is watching a black frame for this.
                self.perspective.surfaces.wake.notify_one();
                return false;
            }
        };
        // Point the pair at the host's ring; the stream has been priming
        // on an empty one since the session was built.
        if let Some(into) = self.audio.lock().unwrap().take() {
            pb.play_audio(self.seat.clone(), into);
        }
        // Show the primed first frame while paused-at-start or still
        // spinning up.
        self.perspective.publish_frames(&pb.frames());
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
        self.speed.set_custom_screen_active(pb.either_player_in_custom_screen());
        self.perspective.publish_frames(&pb.frames());
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
    pub(super) playhead: Playhead,
    pub(super) speed: Arc<SpeedControl>,
    pub(super) paused: Arc<crate::PauseGate>,
    pub(super) cancel: Arc<AtomicBool>,
    pub(super) booted: bool,
    /// The engine playback runs on, for its readiness gate — same
    /// story as the PvP boot's.
    pub(super) backend: &'static (dyn tango_match::Backend + Send + Sync),
}

impl DriveWorker {
    /// Boot the display pair if this is the first tick. Blocks for the
    /// priming walk; `false` if it failed, which ends the session.
    fn boot_if_needed(&mut self) -> bool {
        if self.booted {
            return true;
        }
        self.booted = true;
        let ok = self.playhead.boot();
        // Published either way, and after the boot has put its first
        // frame up: the wait is over even when it ended badly, and a
        // notice still saying "starting" would be claiming progress
        // that isn't coming. What went wrong travels in `prime_error`.
        self.playhead.booted.store(true, Ordering::Release);
        ok
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
        // What `prepare` started may still be coming up (a browser
        // engine's worker threads finish starting between ticks) — the
        // boot would spin on threads that can never arrive, so it
        // waits the gate out. Playback shows its booting notice
        // meanwhile.
        if !self.booted && !self.backend.ready(4) {
            return true;
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
        self.speed.fps_target()
    }
}

/// The seek loop: chases the targets the transport bar requests.
pub struct SeekWorker {
    pub(super) seek: Arc<SeekController>,
    pub(super) playback: SharedPlayback,
    pub(super) cursor: Arc<AtomicU32>,
    pub(super) paused: Arc<crate::PauseGate>,
    pub(super) perspective: Perspective,
    pub(super) speed: Arc<SpeedControl>,
}

impl SeekWorker {
    /// Advance an in-flight chase by at most `budget` ticks, starting
    /// one if a request is pending. `true` while a chase is still
    /// walking — a pumped host should come straight back rather than
    /// advancing playback underneath it.
    pub fn step(&self, budget: u32) -> bool {
        let mut guard = self.playback.lock().unwrap();
        let Some(pb) = guard.as_mut() else { return false };
        let working = pb.seek_step(
            &self.seek,
            budget,
            &mut |tick| self.cursor.store(tick, Ordering::Relaxed),
            &mut |frames| self.perspective.publish_frames(frames),
            &mut || self.paused.set(false),
        ) == tango_match::SeekStep::Working;
        self.speed.set_custom_screen_active(pb.either_player_in_custom_screen());
        working
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
/// match-stats analysis. Where the engine allows, that pair opens by
/// landing on the display pair's primed first state rather than
/// walking a prime of its own.
pub struct PrefetchWorker {
    pub(super) set: Arc<tango_match::ReplaySet>,
    /// The session's mark list, when the host didn't already know where
    /// the rounds fall and the pass has to find them.
    pub(super) round_marks: Option<Arc<Mutex<Vec<u32>>>>,
    /// The session's progress mirror ([`Engine::prefetch_progress`]),
    /// refreshed from the pass once per slice — like the marks.
    pub(super) progress: Arc<AtomicU32>,
    pub(super) cancel: Arc<AtomicBool>,
    pub(super) pass: Option<tango_match::StatsPass>,
    /// What the pass produced, kept so a host can take it after the
    /// loop.
    pub(super) finished: Option<tango_match::analysis::MatchStats>,
    /// The pass finished, failed, or was cancelled — don't reopen it.
    pub(super) done: bool,
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
            // Reuses the display pair's primed state where the engine
            // allows, so the open blocks until the drive worker's boot
            // lands — the same second or two a walk of its own costs,
            // minus the walk.
            match self.set.stats_reusing_playback() {
                Ok(mut pass) => {
                    // The pass reports every tick into the session's
                    // cell from here on, so the scrub bar's shading
                    // tracks it continuously — a slice is sized for how
                    // often this loop wants control back, which is far
                    // coarser than what a bar should move in.
                    pass.report_progress_into(self.progress.clone());
                    self.pass = Some(pass);
                }
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
        // No progress store here: the pass publishes its own, per tick.
        match pass.step(budget) {
            Ok(true) => {
                self.mirror_marks();
                true
            }
            Ok(false) => {
                self.mirror_marks();
                self.finished = self.pass.take().and_then(|p| p.finish());
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

    /// Copy whatever round boundaries the pass has found so far into the
    /// session's list, so the scrub bar picks each one up as the pass
    /// crosses it rather than all at once at the end. A fold in progress
    /// is a truthful prefix — the boundaries it has are real, and the
    /// ones it hasn't reached simply aren't drawn yet.
    fn mirror_marks(&self) {
        let (Some(into), Some(pass)) = (self.round_marks.as_ref(), self.pass.as_ref()) else {
            return;
        };
        let Some(found) = pass.preview().map(|s| s.round_marks()) else {
            return;
        };
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
    pub(super) drive: DriveWorker,
    pub(super) seek: SeekWorker,
    pub(super) prefetch: PrefetchWorker,
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
