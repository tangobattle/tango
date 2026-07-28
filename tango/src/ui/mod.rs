//! The look-and-feel toolkit every screen draws from: reusable
//! [`widgets`], the [`style`] constants and [`theme`] palettes they
//! render with, and the bits of transient visual state that don't
//! belong to any one view ([`anim`] timelines, [`copy_feedback`]'s
//! "Copied!" flash).
//!
//! The toolkit itself lives in the `tango-ui` crate — the gamesupport
//! UI layer draws with the same widgets — and is re-exported here so
//! app code keeps its `crate::ui::*` paths. Only [`theme`]'s
//! config-reading half stays local.

pub use tango_ui::{anim, copy_feedback, style};

/// The widget toolkit, plus the app-side [`matchup`] cooking that feeds
/// its match-analysis chart — one namespace, so call sites read
/// `widgets::cook_hp_rounds` next to `widgets::hp_match_graph`.
pub mod widgets {
    pub use super::matchup::*;
    pub use tango_ui::widgets::*;
}

mod matchup;

pub mod theme;
