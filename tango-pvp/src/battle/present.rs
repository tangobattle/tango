//! Cross-thread hand-off from the live (headless) core to the display core.
//!
//! In the presentation-buffer model the live core runs the rollback netcode at
//! the network frontier with video/audio disabled, while a second *display*
//! core renders what the player sees, trailing `frame_delay` frames behind. The
//! delay *mitigates* rollback: the live Fastforwarder already re-simulates
//! `[committed_tick, frontier]` every frame, so it passes through
//! `frontier - frame_delay` on the way to the frontier — we capture that
//! `present_state` and render it.
//!
//! The live core is the sole writer, publishing one present_state per emulated
//! frame; the display core is the sole reader, popping one in order per frame
//! and re-serving the held frame on underrun.
//!
//! **KNOWN ISSUE (under investigation):** the display core is audio-driven (cpal
//! pulls its samples at the host clock) with no real-time cap (`videoSync=false`,
//! `audioSync=true`), so it free-runs ~5–8% faster than the live produces
//! (measured display ~64fps vs live ~60, the live being implicitly network-paced).
//! It drains the queue and re-serves frames, replaying a frame of audio ~6×/s
//! (audible as crunch, worst on bn3). Slaving the consume to the live was tried
//! and reverted — it just starves cpal instead. The real lever is the display's
//! audio resampler rate (see `tango/src/audio.rs` `MGBAStream::fill`); the
//! temporary `audio[...]`/`present ...` logs are in place to pin it down.

use std::sync::Arc;

use parking_lot::Mutex as PlMutex;

/// What the per-game display traps need to render the live core's published
/// frames. Cheap to clone — handed to `Hooks::display_traps`.
pub type DisplayHandle = Arc<PresentationChannel>;

/// Soft cap on the runahead queue. The depth normally hovers near
/// `PRIME_FRAMES`; this just bounds memory / catch-up if the display ever falls
/// far behind (oldest frames drop, a one-time visible skip).
const QUEUE_CAP: usize = 32;
/// How many frames the live builds before the display starts presenting — the
/// runahead cushion. The display core is audio-driven with no real-time cap, so
/// it runs a few % above 60fps while the live is steady at ~60; when the queue
/// drains the display re-serves (repeats) the held frame. KNOWN ISSUE: this
/// repeat is audible as crunch (worst on bn3) and is not yet solved — see the
/// module docs. Adds `frame_delay + PRIME_FRAMES` of display latency (local
/// input still reaches the live core immediately). NOT `frame_delay` (that's
/// realized by *which tick* the live captures).
const PRIME_FRAMES: usize = 2;

/// The hand-off channel: a [`PresentationBuffer`] behind a mutex. Wraps the
/// buffer so the live (publish) and display (advance) sides share one lock.
pub struct PresentationChannel {
    buf: PlMutex<PresentationBuffer>,
}

impl PresentationChannel {
    pub fn new() -> Self {
        Self {
            buf: PlMutex::new(PresentationBuffer::new()),
        }
    }

    /// Push the live FF's `present_state` (at display tick `tick`) for the
    /// current battle frame. Called once per live frame from the round path.
    pub fn publish(&self, tick: u32, state: Box<mgba::state::State>) {
        self.buf.lock().publish(tick, state);
    }

    /// Load the next present frame on the display core. Pops the next queued
    /// frame in order; on underrun re-serves the held frame. `f` runs with the
    /// frame to load while the lock is held. Returns `None` only during the
    /// pre-publish boot (nothing to show yet).
    pub fn advance_blocking<R>(&self, f: impl FnOnce(&mgba::state::State) -> R) -> Option<R> {
        let mut buf = self.buf.lock();

        if !buf.primed {
            if buf.queue.len() < PRIME_FRAMES {
                return None;
            }
            buf.primed = true;
        }

        if let Some((tick, state)) = buf.queue.pop_front() {
            if buf.stalled {
                log::warn!("present underrun END: resuming at tick {}", tick);
                buf.stalled = false;
            }
            if let Some((prev, _)) = &buf.last {
                let delta = tick as i64 - *prev as i64;
                if delta != 1 {
                    log::warn!("present ADVANCE non-consecutive: {} -> {} (delta {})", prev, tick, delta);
                }
            }
            buf.last = Some((tick, state));
        } else if !buf.stalled {
            let held = buf.last.as_ref().map(|(t, _)| *t).unwrap_or(0);
            log::warn!("present underrun at tick {} (display outrunning live)", held);
            buf.stalled = true;
        }
        buf.last.as_ref().map(|(_, s)| f(&**s))
    }

    /// Once-per-live-frame query for the live frame_callback (see
    /// [`PresentationBuffer::take_battle_published`]).
    pub fn take_battle_published(&self) -> bool {
        self.buf.lock().take_battle_published()
    }

    /// Whether the display has started presenting (runahead primed).
    pub fn is_presenting(&self) -> bool {
        self.buf.lock().is_presenting()
    }

    /// INSTRUMENTATION (temporary): current queued runahead depth.
    pub fn depth(&self) -> usize {
        self.buf.lock().depth()
    }
}

impl Default for PresentationChannel {
    fn default() -> Self {
        Self::new()
    }
}

/// In-order runahead queue from the live core to the display core. The live
/// core publishes one present_state per frame; the display pops exactly one per
/// frame **in order**, so it always loads consecutive ticks. Guarded by
/// [`PresentationChannel`].
pub struct PresentationBuffer {
    queue: std::collections::VecDeque<(u32, Box<mgba::state::State>)>,
    /// Last frame handed out (with its present tick), re-served only when the
    /// live stalls so the display stays pinned rather than running past it.
    last: Option<(u32, Box<mgba::state::State>)>,
    /// False until the queue first fills to `PRIME_FRAMES` (the live core
    /// building its runahead); the display shows nothing until then.
    primed: bool,
    /// Set by [`publish`] and cleared each live frame by
    /// [`take_battle_published`]; lets the live frame_callback tell whether the
    /// in-battle path published this frame (display on screen) or not (boot /
    /// interlude / comm-error screen, where the live core is shown directly).
    battle_published: bool,
    /// INSTRUMENTATION (temporary): last tick handed to [`publish`], to flag a
    /// stuttering producer cadence (present_target not advancing 1/frame).
    last_published_tick: Option<u32>,
    /// Whether the display is currently held because the live stopped
    /// publishing — edge-logged to avoid spam.
    stalled: bool,
}

impl PresentationBuffer {
    pub fn new() -> Self {
        Self {
            queue: std::collections::VecDeque::new(),
            last: None,
            primed: false,
            battle_published: false,
            last_published_tick: None,
            stalled: false,
        }
    }

    fn publish(&mut self, tick: u32, state: Box<mgba::state::State>) {
        if let Some(prev) = self.last_published_tick {
            let delta = tick as i64 - prev as i64;
            if delta != 1 {
                log::warn!("present PUBLISH non-consecutive: {} -> {} (delta {})", prev, tick, delta);
            }
        }
        self.last_published_tick = Some(tick);
        self.queue.push_back((tick, state));
        while self.queue.len() > QUEUE_CAP {
            if let Some((t, _)) = self.queue.pop_front() {
                log::warn!("present overflow: dropped tick {} (display far behind)", t);
            }
        }
        self.battle_published = true;
    }

    fn take_battle_published(&mut self) -> bool {
        std::mem::replace(&mut self.battle_published, false)
    }

    fn is_presenting(&self) -> bool {
        self.primed
    }

    fn depth(&self) -> usize {
        self.queue.len()
    }
}

impl Default for PresentationBuffer {
    fn default() -> Self {
        Self::new()
    }
}
