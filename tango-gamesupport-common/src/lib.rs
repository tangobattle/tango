//! The private gamesupport layer.
//!
//! Always on: [`dataview`], the save/ROM parsing substrate every
//! game's dataview crate implements against — headless builds (pvp
//! probes, engine hosts) get save parsing with no UI toolkit linked.
//!
//! Behind the `ui` feature, the save editor and its supporting cast:
//! [`editor`] (the per-game interface, the shell implementing the
//! public `tango_gamesupport::SaveEditor` embedding API, the view
//! components, and the `OpenSave` baking), [`model`] (the save model +
//! staged edits it operates on), and [`i18n`] (the editor's fluent
//! bundle). Drawing substrate comes from the game-agnostic `tango-ui`
//! toolkit: [`widgets`] and [`style`] re-export it alongside the few
//! pieces only the editor uses, and [`anim`]/[`copy_feedback`] pass
//! straight through — so module paths inside read `crate::widgets` /
//! `crate::style` / etc. either way.

pub mod dataview;

#[cfg(feature = "ui")]
pub use tango_ui::{anim, copy_feedback};
#[cfg(feature = "ui")]
pub mod style;
#[cfg(feature = "ui")]
pub mod widgets;

#[cfg(feature = "ui")]
pub mod editor;
#[cfg(feature = "ui")]
pub mod i18n;
#[cfg(feature = "ui")]
pub mod model;
