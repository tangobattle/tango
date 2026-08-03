//! The melonDS backend: [`tango_match::Link`] over a pair of emulated
//! DSes on emulated local wireless.
//!
//! This is one half of what a DS game needs. The engine-specific half
//! is here — how a pair ticks, snapshots, restores and draws — while
//! the game-specific half (priming a link into that game's link battle)
//! stays in the game's own crate and arrives through
//! [`GameSupport`](backend::GameSupport).
//!
//! The pieces:
//!
//! - [`link`]: the [`tango_match::Link`] implementation — the pair as
//!   the seam's rollback unit — plus the console's constants (screens,
//!   framerate, sample rate).
//! - [`solo`]: one DS booted alone, as the seam's
//!   [`Console`](tango_match::Console).
//! - [`backend`]: the [`tango_match::Backend`] a game registration
//!   holds — netplay, solo, and replay playback all come out of it.
//!
//! Re-exports the pieces a game crate needs so it can depend on this
//! rather than on the emulator directly.
//!
//! [`link`]: mod@link

pub mod backend;
pub mod link;
pub mod solo;

pub use backend::{mute_sequences, DsBackend, GameSupport};
pub use link::{DsScreen, Link, Screens, EXPECTED_FPS, SAMPLE_RATE};
pub use solo::SoloConsole;

/// Re-exported so a game crate can name a console's own input word
/// without depending on the emulator crates itself.
pub use melonds_rollback::Input;

/// One console of a pair. A game crate needs this to reach past the
/// link when priming: execution traps are installed per console.
pub use melonds::Nds;
