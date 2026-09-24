//! The iced look-and-feel the desktop app and the save editor share: the
//! [`widgets`] and [`style`] metrics drawn on both sides of the
//! gamesupport boundary, the [`anim`] timelines they move on, and
//! [`copy_feedback`]'s "Copied!" flash. The browser app draws with
//! Dioxus and doesn't use it.
//!
//! Strictly the overlap. The app (`tango::ui`) and the save-editor layer
//! (`tango-gamesupport-common-ui::editor`) each own their own `widgets` /
//! `style` / `anim` modules that re-export these and add whatever only
//! they use, so nothing one-sided accumulates here — and this crate
//! carries no game-support knowledge at all.

pub mod anim;
pub mod copy_feedback;
pub mod style;
pub mod widgets;
