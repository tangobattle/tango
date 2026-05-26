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
/// runahead cushion absorbing producer/consumer jitter. Adds
/// `frame_delay + PRIME_FRAMES` of display latency (local input still reaches
/// the live core immediately). NOT `frame_delay` (that's realized by *which
/// tick* the live captures).
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

    /// Push the live FF's `present_state` for the current battle frame. Called
    /// once per live frame from the round path.
    pub fn publish(&self, state: Box<mgba::state::State>) {
        self.buf.lock().publish(state);
    }

    /// Load the next present frame on the display core. Pops the next queued
    /// frame in order; on underrun re-serves the held frame. `f` runs with the
    /// frame to load while the lock is held. Returns `None` only during the
    /// pre-publish boot (nothing to show yet).
    pub fn advance<R>(&self, f: impl FnOnce(&mgba::state::State) -> R) -> Option<R> {
        let mut buf = self.buf.lock();

        if !buf.primed {
            if buf.queue.len() < PRIME_FRAMES {
                return None;
            }
            buf.primed = true;
        }

        if let Some(state) = buf.queue.pop_front() {
            buf.last = Some(state);
        }
        buf.last.as_deref().map(f)
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
}

impl Default for PresentationChannel {
    fn default() -> Self {
        Self::new()
    }
}

/// In-order runahead queue from the live core to the display core. The live
/// core publishes one present_state per frame; the display pops exactly one per
/// frame in order. Guarded by [`PresentationChannel`].
pub struct PresentationBuffer {
    queue: std::collections::VecDeque<Box<mgba::state::State>>,
    /// Last frame handed out, re-served on underrun so the display stays pinned
    /// rather than running past the live frontier.
    last: Option<Box<mgba::state::State>>,
    /// False until the queue first fills to `PRIME_FRAMES` (the live core
    /// building its runahead); the display shows nothing until then.
    primed: bool,
    /// Set by [`publish`] and cleared each live frame by
    /// [`take_battle_published`]; lets the live frame_callback tell whether the
    /// in-battle path published this frame (display on screen) or not (boot /
    /// interlude / comm-error screen, where the live core is shown directly).
    battle_published: bool,
}

impl PresentationBuffer {
    pub fn new() -> Self {
        Self {
            queue: std::collections::VecDeque::new(),
            last: None,
            primed: false,
            battle_published: false,
        }
    }

    fn publish(&mut self, state: Box<mgba::state::State>) {
        self.queue.push_back(state);
        while self.queue.len() > QUEUE_CAP {
            self.queue.pop_front();
        }
        self.battle_published = true;
    }

    fn take_battle_published(&mut self) -> bool {
        std::mem::replace(&mut self.battle_published, false)
    }

    fn is_presenting(&self) -> bool {
        self.primed
    }
}

impl Default for PresentationBuffer {
    fn default() -> Self {
        Self::new()
    }
}
