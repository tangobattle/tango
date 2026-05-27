//! Cross-thread hand-off from the live (headless) core to the display core.
//!
//! In the presentation-buffer model the live core runs the rollback netcode at
//! the network frontier with video/audio disabled, while a second *display*
//! core renders what the player sees, trailing `presentation_delay` frames
//! behind. That delay *hides* rollback: the live Fastforwarder already
//! re-simulates `[committed_tick, frontier]` every frame, so it passes through
//! `frontier - presentation_delay` on the way to the frontier — we capture that
//! `present_state` and render it. Most mispredictions settle before the display
//! reaches them; a correction reaching deeper than `presentation_delay` still
//! surfaces as a (smaller, rarer) rollback on the display. (The other half of a
//! peer's requested delay, the shared `input_delay`, instead *shrinks* the live
//! rollback window itself — see `Round`.)
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

/// Soft cap on the hand-off queue. The depth normally hovers near 0–1 (the
/// display consumes about as fast as the live core produces); this just bounds
/// memory / catch-up if the display ever falls far behind (oldest frames drop,
/// a one-time visible skip).
const QUEUE_CAP: usize = 32;

/// In-order hand-off queue from the live core to the display core. The live
/// core runs freely, pushing one `present_state` per frame; the display core's
/// clock drives consumption — it pops exactly one per frame **in order**, so it
/// always loads consecutive ticks and its played audio never shifts. The
/// display starts on the first frame published (no runahead reserve), so its
/// latency is exactly `presentation_delay`; the cost is that scheduling jitter
/// can momentarily underrun, re-serving the last frame for a one-frame judder.
pub struct PresentationBuffer {
    queue: std::collections::VecDeque<(u32, Box<mgba::state::State>)>,
    /// Last `(present_tick, frame)` handed out, re-served on underrun so the
    /// display stays pinned rather than running past the live frontier. `Some`
    /// once the display has shown its first frame — doubles as the "presenting"
    /// signal (see [`Self::is_presenting`]).
    last: Option<(u32, Box<mgba::state::State>)>,
    /// Set by [`publish`] (a battle present_state) and cleared each live frame
    /// by [`take_battle_published`]. Lets the live frame_callback tell whether
    /// the in-battle path already published this frame.
    battle_published: bool,
}

impl PresentationBuffer {
    pub fn new() -> Self {
        Self {
            queue: std::collections::VecDeque::new(),
            last: None,
            battle_published: false,
        }
    }

    fn push_capped(&mut self, item: (u32, Box<mgba::state::State>)) {
        self.queue.push_back(item);
        while self.queue.len() > QUEUE_CAP {
            self.queue.pop_front();
        }
    }

    /// Push the live FF's `present_state` (rendered at `present_tick`, i.e.
    /// `frontier - presentation_delay`) for the current battle frame. Called once per
    /// live frame from the round path. Drops the oldest if the display has
    /// fallen `QUEUE_CAP` behind (rare; a one-time catch-up skip).
    pub fn publish(&mut self, present_tick: u32, state: Box<mgba::state::State>) {
        self.push_capped((present_tick, state));
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

    /// Pop the next frame in order for the display to load. `None` only before
    /// the first frame is ever published (display still booting); on underrun
    /// re-serves the last frame. Called once per display frame from its joyflags
    /// trap.
    pub fn advance(&mut self) -> Option<&mgba::state::State> {
        if let Some(next) = self.queue.pop_front() {
            self.last = Some(next);
        }
        self.last.as_ref().map(|(_, state)| state.as_ref())
    }

    /// Whether the display has started presenting (has shown at least one
    /// frame). Gates the display frame_callback's UI push so the hidden boot
    /// stays off-screen.
    pub fn is_presenting(&self) -> bool {
        self.last.is_some()
    }

    /// The present tick (`frontier - presentation_delay`) of the frame the display most
    /// recently loaded. Lets a per-game `send_and_receive` neuter stamp the
    /// matching tick into the canned rx packet (mirroring the primary path)
    /// instead of leaving a stale tick in the rendered frame.
    pub fn current_tick(&self) -> u32 {
        self.last.as_ref().map(|(tick, _)| *tick).unwrap_or(0)
    }
}

impl Default for PresentationBuffer {
    fn default() -> Self {
        Self::new()
    }
}
