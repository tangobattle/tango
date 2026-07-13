//! Live PvP match orchestration.
//!
//! [`Match`] owns the connection-level state: shadow emulator, RNG, sender,
//! replay writer, round counter. It exposes `start_round` (creates a fresh
//! [`Round`]) and `run` (the network receive loop that feeds remote inputs
//! into the in-progress round).
//!
//! [`Round`] owns one round's worth of state: a thin shell around the
//! generic [`getgud::Session`] rollback engine, wiring it to the re-sim
//! [`Stepper`](crate::stepper::Stepper), the opponent co-sim shadow, and
//! the time-sync throttler.

mod match_;
mod round;
mod throttler;
mod world;

pub(crate) use match_::SenderMutex;
pub use match_::{Match, MatchConfig, RoundMetrics};
pub use round::RoundPhase;
pub(crate) use round::Round;
pub use world::{RoundSample, BUTTON_LOCAL_A, BUTTON_LOCAL_B, BUTTON_REMOTE_A, BUTTON_REMOTE_B, NO_CHIP};
pub(crate) use world::{JOY_A, JOY_B};

/// Match-wide identity. Both peers compute these to identical values from the
/// shared protocol state, then carry them through Match → Shadow → Round.
#[derive(Clone, Copy)]
pub struct MatchIdentity {
    pub match_type: (u8, u8),
    pub is_offerer: bool,
    pub local_player_index: u8,
    /// The negotiated match clock: the fixed time every core on both sides —
    /// primary, shadow, and each round's re-sim [`Stepper`](crate::stepper::Stepper)
    /// — pins its cart RTC to (`Core::set_rtc_fixed`), so RTC-reading games
    /// (exe45) stay deterministic. Also recorded as the replay's `metadata.ts`
    /// so playback pins to the identical value.
    pub rtc_time: std::time::SystemTime,
}

/// Replay sink: a writer, or none if not recording.
pub struct ReplayConfig {
    pub writer: Option<crate::replay::Writer>,
}

/// GBA video framerate in frames per second.
pub const EXPECTED_FPS: f32 = 16777216.0 / 280896.0;

/// In-match input-buffer budget — two coupled depths expressed as one.
///
/// The depth the session waits for before declaring a dead link and the
/// rollback horizon the engine bails at used to be tuned by hand (the former as
/// a silence *duration*) and could drift apart. [`RECONNECT_QUEUE_LENGTH`] is
/// now the single knob; [`MAX_QUEUE_LENGTH`] (the horizon) is *derived* from it,
/// so the horizon can't end up smaller than the depth it has to out-cover.
///
/// Why watch the queue and not elapsed silence: a dead link keeps the sim
/// committing ~one local input per displayed frame (the throttler caps its
/// slowdown, so it never fully stalls) with nothing from the peer to match them
/// against, so the local input queue climbs steadily. The session polls that
/// depth directly and pauses to reconnect once it reaches
/// [`RECONNECT_QUEUE_LENGTH`]. Measuring the very resource that overflows — not
/// a time proxy for it — means the trip can't drift from the bail no matter how
/// fast the throttled sim actually grows the queue: the watchdog always fires a
/// fixed margin below the horizon.
///
/// That margin is [`STALL_HEADROOM`]: it covers the watchdog's poll interval and
/// the frame or two `pause()` takes to land, plus a safety factor — sized so the
/// overflow bail can never beat the watchdog + pause to the punch.
///
/// The session reads [`RECONNECT_QUEUE_LENGTH`] back to drive its watchdog.
/// Lower it to trip reconnect sooner (the horizon shrinks with it); raise it to
/// ride out longer blips (the horizon grows). Nothing else to retune.
///
/// 180 frames ≈ 3 s of play (at 60 fps, just above [`EXPECTED_FPS`]).
pub const RECONNECT_QUEUE_LENGTH: usize = 180;

/// Slack between the reconnect trip depth and the hard overflow bail — see
/// [`RECONNECT_QUEUE_LENGTH`]. It need not match the trip depth itself; the slop
/// it has to cover is a handful of frames, far short of the depth's worth of
/// growth. 90 frames ≈ 1.5 s.
const STALL_HEADROOM: usize = 90;

/// Per-side input-queue capacity (the rollback horizon): how many local inputs
/// may sit unmatched against remote ones (and vice versa) before the engine
/// bails and cancels the match. Derived from [`RECONNECT_QUEUE_LENGTH`] — see
/// that constant for the budget. Public because it's the backpressure bound
/// other layers size against (the host's send pump, rennet's redundancy window
/// and reorder buffer); anything queueing inputs upstream can hold a bit more
/// and rely on this bail — or the session's earlier reconnect pause — firing
/// first.
pub const MAX_QUEUE_LENGTH: usize = RECONNECT_QUEUE_LENGTH + STALL_HEADROOM;

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
