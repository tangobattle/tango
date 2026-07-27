//! The save-view state layer (tabs, `State`, `Action`, `Outcome`) is
//! public API on `tango_gamesupport::save_view` — the minimum that
//! types `Game::save_ui`. The rendering shell and shared components are
//! private gamesupport knowledge in `tango-gamesupport-common`, which
//! re-exports the state layer, so one glob here covers both.

pub use tango_gamesupport_common::save_view::*;
