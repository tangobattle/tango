//! The user's game library and its persistence, frontend-agnostic.
//!
//! * [`game`]: the registry of supported games everything else is keyed
//!   by, and the game-name localizer.
//! * [`scanner`]: the shared fingerprint-gated rescan machinery the
//!   content scans below build on.
//! * [`rom`] / [`save`] / [`patch`] / [`replays`]: one module per kind of
//!   content the library folders hold.
//! * [`rom_overrides`]: patch-driven chip/navicust/patch-card asset
//!   overrides layered onto a ROM's assets.
//! * [`bnlc`]: Battle Network Legacy Collection (Steam) discovery, an
//!   extra source of ROMs — native only, and absent from a wasm build.
//! * [`config`]: the persisted settings model.
//!
//! Nothing here knows about a UI toolkit, and nothing here touches the
//! filesystem or the network directly: [`storage::Storage`] and
//! [`http::Http`] are the two seams, and a frontend supplies both. The
//! `native` feature (on by default) provides `std::fs` and reqwest
//! implementations; a browser build turns it off and hands in OPFS and
//! `fetch` instead.

pub mod config;
pub mod game;
pub mod http;
pub mod lang;
pub mod marker;
pub mod patch;
pub mod replays;
pub mod rom;
pub mod rom_overrides;
pub mod save;
pub mod scanner;
pub mod storage;

// Steam discovery: no meaning in a browser, and it pulls in steamlocate.
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub mod bnlc;

pub use storage::Storage;
