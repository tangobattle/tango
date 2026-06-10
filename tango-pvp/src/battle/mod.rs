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
