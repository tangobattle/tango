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
mod present;
mod round;
pub mod throttler;
mod types;

pub use match_::{Match, ThrottlerFactory};
pub use present::{DisplayHandle, PresentationBuffer};
pub use round::Round;
pub use types::{BattleOutcome, CommittedState, MatchIdentity, ReplayConfig};

/// GBA video framerate in frames per second.
pub const EXPECTED_FPS: f32 = 16777216.0 / 280896.0;

/// Inclusive bounds for the local presentation delay (frames the display core
/// trails the network frontier). 0 disables the delay (display follows the
/// frontier directly); the UI sliders and config clamp to this range.
pub const MIN_FRAME_DELAY: u32 = 0;
pub const MAX_FRAME_DELAY: u32 = 10;
