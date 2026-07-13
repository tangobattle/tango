//! The on-disk game library: what the user has, found and modeled.
//!
//! * [`game`]: the registry of supported games everything else is
//!   keyed by.
//! * [`scanner`]: the shared fingerprint-gated rescan machinery the
//!   content scans below build on.
//! * [`rom`] / [`save`] / [`patch`] / [`replays`]: one module per
//!   kind of content the library folders hold.
//! * [`rom_overrides`]: patch-driven chip/navicust/patch-card asset
//!   overrides layered onto a ROM's assets.
//! * [`bnlc`]: Battle Network Legacy Collection (Steam) discovery,
//!   an extra source of ROMs + chrome assets.

pub mod bnlc;
pub mod game;
pub mod patch;
pub mod replays;
pub mod rom;
pub mod rom_overrides;
pub mod save;
pub mod scanner;
