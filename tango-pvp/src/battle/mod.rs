//! Live PvP match orchestration.
//!
//! [`Match`] owns the connection-level state: shadow emulator, RNG, sender,
//! replay writer, round counter. It exposes `start_round` (creates a fresh
//! [`Round`]) and `run` (the network receive loop that feeds remote inputs
//! into the in-progress round).
//!
//! [`Round`] owns one round's worth of state: the local input queue, the
//! Fastforwarder instance that drives per-frame simulation, and the helpers
//! that wire remote-side prediction into FF.

mod match_;
mod round;
mod throttler;
mod world;

pub use match_::{Match, RoundMetrics};
pub(crate) use match_::SenderMutex;
pub(crate) use round::Round;

/// Match-wide identity. Both peers compute these to identical values from the
/// shared protocol state, then carry them through Match → Shadow → Round.
#[derive(Clone, Copy)]
pub struct MatchIdentity {
    pub match_type: (u8, u8),
    pub is_offerer: bool,
    pub local_player_index: u8,
}

/// Replay sink: a writer, or none if not recording.
pub struct ReplayConfig {
    pub writer: Option<crate::replay::Writer>,
}

/// GBA video framerate in frames per second.
pub const EXPECTED_FPS: f32 = 16777216.0 / 280896.0;

/// In-match input-buffer budget — two coupled numbers expressed as one.
///
/// The link-liveness window the session waits before declaring a dead link and
/// the rollback horizon the engine bails at used to be tuned by hand and could
/// drift apart. [`SILENCE_WINDOW`] is now the single knob; [`MAX_QUEUE_LENGTH`]
/// (the horizon) is *derived* from it, so the horizon can't end up smaller than
/// the window it has to out-cover.
///
/// Why it has to out-cover it: a stalled link keeps the sim committing ~one
/// local input per displayed frame (the throttler caps its slowdown, so it
/// never fully stalls) right up until the session's watchdog notices the
/// silence and pauses the sim. So the queue grows by roughly a window's worth of
/// frames — plus the watchdog's poll/pause slop and whatever speculative lead
/// was already standing when the link died — before it can possibly stop.
/// Sizing the horizon at the window plus [`STALL_HEADROOM_MS`] keeps the
/// overflow bail a backstop the watchdog beats every time, rather than a second
/// timeout racing it.
///
/// The session reads [`SILENCE_WINDOW`] back as its `RECONNECT_SILENCE`. Shorten
/// the window to trip reconnect faster (the horizon shrinks with it); lengthen
/// it to ride out longer blips (the horizon grows). Nothing else to retune.
pub const SILENCE_WINDOW: std::time::Duration = std::time::Duration::from_millis(SILENCE_WINDOW_MS);
const SILENCE_WINDOW_MS: u64 = 3000;

/// Slack folded into [`MAX_QUEUE_LENGTH`] on top of [`SILENCE_WINDOW`]: covers
/// the watchdog's poll interval and the frame or two `pause()` takes to land,
/// the speculative lead standing at the drop, and a safety factor over all of
/// it — sized so the overflow bail can never beat the watchdog + pause to the
/// punch (it need not exceed the window itself; the slop it has to cover is a
/// handful of frames, far short of the window's worth of growth).
const STALL_HEADROOM_MS: u64 = 1500;

/// Per-side input-queue capacity (the rollback horizon): how many local inputs
/// may sit unmatched against remote ones (and vice versa) before the engine
/// bails and cancels the match. Derived from [`SILENCE_WINDOW`] — see that
/// constant for the budget. Public because it's the backpressure bound other
/// layers size against (the host's send pump, rennet's redundancy window and
/// reorder buffer); anything queueing inputs upstream can hold a bit more and
/// rely on this bail — or the session's earlier silence pause — firing first.
pub const MAX_QUEUE_LENGTH: usize = frames_for_ms(SILENCE_WINDOW_MS) + frames_for_ms(STALL_HEADROOM_MS);

/// Frames spanned by `ms` of wall-clock, rounding fps *up* to 60 (above
/// [`EXPECTED_FPS`]) and rounding the result up: overestimating growth is the
/// safe direction when sizing a capacity from a duration.
const fn frames_for_ms(ms: u64) -> usize {
    ((ms * 60 + 999) / 1000) as usize
}

/// Inclusive bounds for a side's `frame_delay`, which is realized purely as
/// local frame delay (how far the display trails the netcode frontier).
/// Each side picks its own; there's no negotiation. The lobby slider and config
/// clamp to this range.
pub const MIN_FRAME_DELAY: u32 = 2;
pub const MAX_FRAME_DELAY: u32 = 10;

pub fn suggest_frame_delay(rtt: std::time::Duration) -> u32 {
    let one_way_frames = (rtt.as_millis() * 60 / 2 / std::time::Duration::from_secs(1).as_millis()) as i32;
    (one_way_frames + 1).clamp(MIN_FRAME_DELAY as i32, MAX_FRAME_DELAY as i32) as u32
}
