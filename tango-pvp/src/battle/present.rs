//! Cross-thread hand-off from the live (headless) core to the display core.
//!
//! In the presentation-buffer model the live core runs the rollback netcode at
//! the network frontier with video/audio disabled, while a second *display*
//! core renders what the player sees, trailing `frame_delay` frames behind. The
//! delay *mitigates* rollback: the live Fastforwarder already re-simulates
//! `[committed_tick, frontier]` every frame, so it passes through
//! `frontier - frame_delay` on the way to the frontier — we capture that
//! `present_state` and render it. Most mispredictions settle before the display
//! reaches them; a correction reaching deeper than `frame_delay` still surfaces
//! as a (smaller, rarer) rollback on the display.
//!
//! The live core is the sole writer, publishing one [`Frame`] per emulated
//! frame; the display core is the sole reader. No input log, shadow, or second
//! FF is involved — the display just renders the states the live side hands it.

use std::sync::Arc;

use parking_lot::Mutex as PlMutex;

/// What the per-game display traps need to render the live core's published
/// frames: the hand-off buffer. Cheap to clone — handed to
/// `Hooks::display_traps`.
pub type DisplayHandle = Arc<PlMutex<PresentationBuffer>>;

/// Soft cap on the runahead queue. The depth normally hovers near
/// `PRIME_FRAMES`; this just bounds memory / catch-up if the display ever falls
/// far behind (oldest frames drop, a one-time visible skip).
const QUEUE_CAP: usize = 32;
/// Producer/consumer handoff depth between the live and display threads. This
/// is NOT `frame_delay` — that's already realized by the FF capturing
/// `present_state` at `frontier - frame_delay` (the live core's runahead lives
/// in *which tick* it captures, recomputed each frame). This buffer only
/// absorbs thread-scheduling jitter / an occasional heavier live frame so the
/// display never underruns; its size is governed by that jitter (a constant),
/// not by how much rollback mitigation was requested. 2 is the minimum for a
/// stable lock-free double-buffer (1 oscillates 0↔1 and underruns on any
/// hiccup; 2 keeps one frame in reserve).
const PRIME_FRAMES: usize = 2;

/// In-order runahead queue from the live core to the display core. The live
/// core is the runahead: it runs freely, pushing one `present_state` per frame
/// and staying ~`PRIME_FRAMES` ahead. The display core's clock drives
/// consumption — it pops exactly one per frame **in order**, so it always loads
/// consecutive ticks and its played audio never shifts. The buffer absorbs the
/// live core's rollback spikes; only a sustained underrun repeats a frame.
pub struct PresentationBuffer {
    queue: std::collections::VecDeque<Box<mgba::state::State>>,
    /// Last frame handed out, re-served on underrun so the display stays pinned
    /// rather than running past the live frontier.
    last: Option<Box<mgba::state::State>>,
    /// False until the queue first fills to `PRIME_FRAMES` (the live core
    /// building its runahead); the display shows nothing until then.
    primed: bool,
    /// Set by [`publish`] (a battle present_state) and cleared each live frame
    /// by [`should_follow_live`]. Lets the live frame_callback tell whether the
    /// in-battle path already published this frame.
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

    fn push_capped(&mut self, state: Box<mgba::state::State>) {
        self.queue.push_back(state);
        while self.queue.len() > QUEUE_CAP {
            self.queue.pop_front();
        }
    }

    /// Push the live FF's `present_state` for the current battle frame. Called
    /// once per live frame from the round path. Drops the oldest if the display
    /// has fallen `QUEUE_CAP` behind (rare; a one-time catch-up skip).
    pub fn publish(&mut self, state: Box<mgba::state::State>) {
        self.push_capped(state);
        self.battle_published = true;
    }

    /// Once-per-live-frame query for the live frame_callback: returns whether
    /// the round path published a battle present_state this frame (i.e. we're
    /// in an active battle, so the display core is what's shown), and clears the
    /// flag. False means the live core is outside the battle loop — boot, the
    /// between-battles interlude, or the communication-error screen on
    /// disconnect — where the display core can't follow, so the live core is
    /// shown directly instead.
    pub fn take_battle_published(&mut self) -> bool {
        std::mem::replace(&mut self.battle_published, false)
    }

    /// Pop the next frame in order for the display to load. `None` until the
    /// runahead has primed (display still booting); on underrun re-serves the
    /// last frame. Called once per display frame from its joyflags trap.
    pub fn advance(&mut self) -> Option<&mgba::state::State> {
        if !self.primed {
            if self.queue.len() < PRIME_FRAMES {
                return None;
            }
            self.primed = true;
        }
        if let Some(next) = self.queue.pop_front() {
            self.last = Some(next);
        }
        self.last.as_deref()
    }

    /// Whether the display has started presenting (runahead primed). Gates the
    /// display frame_callback's UI push so the hidden boot stays off-screen.
    pub fn is_presenting(&self) -> bool {
        self.primed
    }
}

impl Default for PresentationBuffer {
    fn default() -> Self {
        Self::new()
    }
}
