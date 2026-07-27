//! The save view lives in `tango-gamesupport`'s feature-gated `ui`
//! layer; each game carries its own editor as a feature-gated `ui`
//! module, and `Game::save_ui` points at it — correctly typed, present
//! only when the `ui` feature is on. Nothing is left to do here but
//! keep the app's `crate::save_view::*` paths.

pub use tango_gamesupport::save_view::*;
