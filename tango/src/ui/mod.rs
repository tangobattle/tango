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

/// The widget toolkit, plus the gamesupport layer's match-analysis
/// widgets (HP graph, outcome marks, stat cooking) — one namespace so
/// call sites don't care which side of the submodule boundary a widget
/// lives on.
pub mod widgets {
    pub use tango_gamesupport::matchup::*;
    pub use tango_ui::widgets::*;
}

pub mod theme;
