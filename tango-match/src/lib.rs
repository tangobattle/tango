//! The engine-neutral seam every Tango netplay match speaks.
//!
//! A match is two consoles linked together, simulated locally, with the
//! pair as the rollback unit — but *which* consoles, and what links
//! them, is the engine's business. The GBA games run on mgba over an
//! emulated link cable (`tango-backend-mgba`); BN5 Double Team DS runs
//! on melonDS over emulated local wireless (`tango-backend-melonds`).
//!
//! Nothing here names an emulator, so a game pulls in exactly the engine
//! it needs and no other.
//!
//! - [`link`]: [`Link`] — the linked pair — and [`Side`], one console
//!   of any boot; plus [`Backend`], what a `Game` registration holds.
//! - [`audio`]: the ring a simulation pushes its consoles' sound into
//!   and a host's device callback plays out of.
//! - [`engine`]: [`Match`], the rollback loop over any [`Link`] and the
//!   unified session surface a host drives.
//! - [`solo`]: [`Solo`], the single-console ride over any [`Console`].
//! - [`replay`]: [`ReplaySet`], playback + seeking + the statistics
//!   pass over any [`Link`] — an engine contributes only the boot.
//! - [`battle`]: the per-tick stats sample encoding, which is just a
//!   layout — no engine has an opinion about it.
//! - [`input`]: the joyflags input type that lands in replays.
//! - [`throttler`]: the clock-sync governor both engines pace with.
//! - [`keys`]: the joypad bit vocabulary.

pub mod engine;
#[cfg(target_arch = "wasm32")]
pub mod hosting;
pub mod link;
pub mod analysis;
pub mod audio;
pub mod battle;
pub mod input;
pub mod replay;
pub mod seek;
pub mod solo;
pub mod telemetry;
pub mod throttler;

pub use solo::{Console, Solo, SoloConfig};
pub use engine::Match;
pub use audio::{AudioIn, AudioOut};
pub use link::{
    Backend, FrameTiming, Link, PeerRom, Screen, ScreenLayout, SessionMode, Side, Snapshot, StartConfig,
};
pub use replay::{
    BootedReplay, Capture, LiveFrames, Playback, Replay, ReplayBoot, ReplayConfig, ReplaySet, SeekStep, StatsPass,
};
pub use input::HostInput;

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
    /// engine has its own error type and this crate refuses to name
    /// any of them.
    #[error(transparent)]
    Backend(Box<dyn std::error::Error + Send + Sync>),

    /// A ride this game's engine doesn't offer — a netplay-only game
    /// asked for single-player, say. Not a failure: the host reports
    /// the option isn't available and carries on.
    #[error("{0}")]
    Unsupported(&'static str),
}

/// The input bits a match speaks, engine-neutral.
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

    /// DS only, and the one bit here that is not a button: white noise
    /// held on the console's microphone.
    ///
    /// It rides the pad word because everything that has to carry it
    /// already carries that word and nothing else — the netplay wire,
    /// the rollback engine's input queues and its prediction, the replay
    /// stream. A mic channel of its own would be the same bit spelled
    /// four more times, and every one of those places would have to be
    /// taught that a console can be blown into.
    pub const MIC: u32 = 1 << 12;

    /// Every named bit — the widest input this vocabulary can express,
    /// which is also what the netplay wire's input element carries.
    pub const MASK: u32 = 0x1fff;
}
