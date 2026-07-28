//! The look-and-feel toolkit every screen draws from: reusable
//! [`widgets`], the [`style`] metrics and [`theme`] palettes they
//! render with, and the bits of transient visual state that don't
//! belong to any one view ([`anim`] timelines, [`copy_feedback`]'s
//! "Copied!" flash).
//!
//! What the save-editor layer draws with too lives one crate down, in
//! `tango-ui`; the modules here own the app's own half and re-export
//! the shared one, so app code reads `crate::ui::*` for both and never
//! has to know which side a given widget comes from. [`copy_feedback`]
//! is shared whole, so it passes straight through.

pub use tango_ui::copy_feedback;

pub mod anim;
pub mod style;
pub mod theme;
pub mod widgets;

mod matchup;
