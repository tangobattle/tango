//! The private gamesupport layer's save editor and its supporting
//! cast: [`editor`] (the per-game interface, the shell implementing the
//! public `tango_gamesupport::SaveEditor` embedding API, the view
//! components, and the `OpenSave` baking), [`model`] (the save model +
//! staged edits it operates on), and [`i18n`] (the editor's fluent
//! bundle). Drawing substrate comes from the game-agnostic `tango-ui`
//! toolkit: [`widgets`] and [`style`] re-export it alongside the few
//! pieces only the editor uses, and [`anim`]/[`copy_feedback`] pass
//! straight through — so module paths inside read `crate::widgets` /
//! `crate::style` / etc. either way. Likewise [`dataview`] passes the
//! parsing substrate crate through, so save-model code reads
//! `crate::dataview` like it always has.

pub use tango_gamesupport_common_dataview as dataview;
pub use tango_ui::{anim, copy_feedback};

pub mod build;
pub mod editor;
pub mod i18n;
pub mod model;
pub mod style;
pub mod widgets;
