//! The app's look-and-feel toolkit: reusable [`widgets`], the [`style`]
//! constants and [`theme`] decorations they render with, and the bits of
//! transient visual state that don't belong to any one view ([`anim`]
//! timelines, [`copy_feedback`]'s "Copied!" flash).
//!
//! Game-agnostic on purpose: the save-editor layer (in the private
//! gamesupport repo) and the app both draw with this, so the main repo
//! carries no game-support knowledge beyond ROM/save detection.

pub mod anim;
pub mod copy_feedback;
pub mod style;
pub mod theme;
pub mod widgets;
