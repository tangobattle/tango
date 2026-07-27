//! The shared save-editor UI layer.
//!
//! Extracted from the tango app so per-game UI crates (in the
//! `gamesupport` submodule) can render their own save editors without
//! linking the whole app — and so the raw gamesupport crates never link
//! iced at all. Three layers live here:
//!
//! * the widget substrate the whole app draws with ([`widgets`],
//!   [`style`], [`theme`], [`anim`], [`copy_feedback`]) — tango
//!   re-exports these as `crate::ui::*`;
//! * the save-view components ([`save_view`]) plus the baked-art bundle
//!   they draw from ([`loaded::OpenSave`]);
//! * the [`save_ui::SaveUi`] trait a per-game UI crate implements, and
//!   which the shell in [`save_view`] dispatches through.

pub mod anim;
pub mod copy_feedback;
pub mod i18n;
pub mod loaded;
pub mod save_ui;
pub mod save_view;
pub mod style;
pub mod theme;
pub mod widgets;
