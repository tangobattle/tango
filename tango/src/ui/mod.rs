//! The look-and-feel toolkit every screen draws from: reusable
//! [`widgets`], the [`style`] constants and [`theme`] palettes they
//! render with, and the bits of transient visual state that don't
//! belong to any one view ([`anim`] timelines, [`copy_feedback`]'s
//! "Copied!" flash).
//!
//! The toolkit itself lives in the `tango-gamesupport-ui` crate — the
//! per-game save-editor UI crates draw with the same widgets — and is
//! re-exported here so app code keeps its `crate::ui::*` paths. Only
//! [`theme`]'s config-reading half stays local.

pub use tango_gamesupport_ui::{anim, copy_feedback, style, widgets};

pub mod theme;
