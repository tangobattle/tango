//! The engine-neutral seam every Tango netplay match speaks.
//!
//! A match is two consoles linked together, simulated locally, with the
//! pair as the rollback unit — but *which* consoles, and what links
//! them, is the engine's business. The GBA games run on mgba over an
//! emulated link cable (`tango-match-mgba`); BN5 Double Team DS runs on
//! melonDS over emulated local wireless (`tango-match-melonds`).
//!
//! Nothing here names an emulator, so a game pulls in exactly the engine
//! it needs and no other.
//!
//! - [`backend`]: [`Backend`] (generic, per-engine) plus [`MatchFactory`]
//!   and [`RunningMatch`] (object-safe, what a `Game` registration holds
//!   and a host drives).
//! - [`battle`]: the per-tick stats sample encoding, which is just a
//!   layout — no engine has an opinion about it.
//! - [`input`]: the joyflags input type that lands in replays.
//! - [`throttler`]: the clock-sync governor both engines pace with.
//! - [`keys`]: the joypad bit vocabulary.

pub mod audio;
pub mod backend;
pub mod engine;
pub mod analysis;
pub mod battle;
pub mod input;
pub mod seek;
pub mod solo;
pub mod telemetry;
pub mod throttler;

pub use audio::{AudioDrain, AudioPull, Resampled};
pub use solo::{
    PeerRom, ReplayConfig, ReplayFrames, ReplaySet, RunningReplay, RunningSolo, SeekStep, SoloConfig,
    StatsPass,
};
pub use backend::{Backend, MatchFactory, RunningMatch, Screen, ScreenLayout, StartConfig};

/// The clock-sync governor: feed it `skew()` + `speculation_balance()`
/// each frame and shave the returned fps off the tick rate. Shared by
/// every engine, so it lives here rather than in a backend.
pub use throttler::Throttler;

/// Why a match failed to start or advance.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Priming never reached a link battle within the tick bound — a
    /// wedged menu walk, or the wrong ROM/save for the primer.
    #[error("priming did not reach a link battle within {0} ticks")]
    PrimeTimeout(u32),

    /// The caller's cancel flag flipped mid-simulation.
    #[error("cancelled")]
    Cancelled,

    /// Whatever the engine underneath reported. Boxed because every
    /// backend has its own error type and this crate refuses to name
    /// any of them.
    #[error(transparent)]
    Backend(Box<dyn std::error::Error + Send + Sync>),

    /// A ride this game's engine doesn't offer — a netplay-only game
    /// asked for single-player, say. Not a failure: the host reports
    /// the option isn't available and carries on.
    #[error("{0}")]
    Unsupported(&'static str),
}

/// The joypad bits a match speaks, engine-neutral.
///
/// This is the GBA layout, which the DS extends with X and Y — so one
/// vocabulary covers both consoles and hosts need no emulator
/// dependency to name a button.
pub mod keys {
    pub const A: u32 = 1 << 0;
    pub const B: u32 = 1 << 1;
    pub const SELECT: u32 = 1 << 2;
    pub const START: u32 = 1 << 3;
    pub const RIGHT: u32 = 1 << 4;
    pub const LEFT: u32 = 1 << 5;
    pub const UP: u32 = 1 << 6;
    pub const DOWN: u32 = 1 << 7;
    pub const R: u32 = 1 << 8;
    pub const L: u32 = 1 << 9;
    /// DS only.
    pub const X: u32 = 1 << 10;
    /// DS only.
    pub const Y: u32 = 1 << 11;
}
