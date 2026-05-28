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
//! The live core is the sole writer, publishing one frame per emulated frame
//! into a small in-order queue; the display core is the sole reader, popping one
//! per display frame. No input log, shadow, or second FF is involved — the
//! display just loads and renders the states the live side hands it.

use std::sync::Arc;

use parking_lot::Mutex as PlMutex;

/// What the per-game display traps need to render the live core's published
/// frames: the hand-off buffer. Cheap to clone — handed to
/// `Hooks::display_traps`.
pub type DisplayHandle = Arc<PlMutex<PresentationBuffer>>;

/// Max queue length. This is a jitter buffer between the live and display cores,
/// not the presentation delay (that's already baked into *which* tick the FF
/// captures, `frontier - presentation_delay`). Every queued frame is already at
/// the budgeted tick, so any backlog is pure added latency — keep it tiny and
/// governed by scheduling jitter, a small constant. 2 = one hand-off slot + one
/// frame of in-order slack; beyond it the oldest frame drops (a catch-up skip).
const QUEUE_CAP: usize = 2;

/// In-order hand-off queue from the live core to the display core. The live core
/// pushes one `present_state` per frame; the display core pops one per display
/// frame **in order**, so it loads consecutive ticks and its played audio never
/// shifts. The two run on independent ~60 fps clocks; the queue absorbs the
/// jitter between them, bounded by [`QUEUE_CAP`].
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

    /// Push the live FF's `present_state` (rendered at `present_tick`, i.e.
    /// `frontier - presentation_delay`) for the current battle frame. Called once
    /// per live frame from the round path. Drops the oldest queued frame once the
    /// backlog exceeds [`QUEUE_CAP`] (the display fell behind; a catch-up skip).
    pub fn publish(&mut self, present_tick: u32, state: Box<mgba::state::State>) {
        self.queue.push_back((present_tick, state));
        while self.queue.len() > QUEUE_CAP {
            self.queue.pop_front();
        }
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

    /// The present tick (`frontier - presentation_delay`) of the frame the
    /// display most recently loaded. Lets a per-game `send_and_receive` neuter
    /// stamp the matching tick into the canned rx packet (mirroring the primary
    /// path) instead of leaving a stale tick in the rendered frame.
    pub fn current_tick(&self) -> u32 {
        self.last.as_ref().map(|(tick, _)| *tick).unwrap_or(0)
    }
}

impl Default for PresentationBuffer {
    fn default() -> Self {
        Self::new()
    }
}
