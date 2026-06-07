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

pub use match_::Match;
pub use round::Round;

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

/// Save snapshot from the FF, paired with the local emulator's outgoing
/// link-cable packet at the moment of capture. Both Fastforwarder and replay
/// use this.
#[derive(Clone)]
pub struct Snapshot {
    pub state: Box<mgba::state::State>,
    /// `game.current_tick` at the moment the snapshot was captured — i.e. the
    /// tick the game is *about to process next*, an exclusive upper bound of
    /// what's already been simulated. For `Round::settled_state` this is the
    /// display target, capped at `commit_frontier − 1`.
    pub tick: u32,
    pub packet: Vec<u8>,
}

/// GBA video framerate in frames per second.
pub const EXPECTED_FPS: f32 = 16777216.0 / 280896.0;

/// Inclusive bounds for a side's `frame_delay`, which is realized purely as
/// local frame delay (how far the display trails the netcode frontier).
/// Each side picks its own; there's no negotiation. The lobby slider and config
/// clamp to this range.
pub const MIN_FRAME_DELAY: u32 = 2;
pub const MAX_FRAME_DELAY: u32 = 10;
