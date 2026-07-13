//! The look-and-feel toolkit every screen draws from: reusable
//! [`widgets`], the [`style`] constants and [`theme`] palettes they
//! render with, and the bits of transient visual state that don't
//! belong to any one view ([`anim`] timelines, [`copy_feedback`]'s
//! "Copied!" flash).

pub mod anim;
pub mod copy_feedback;
pub mod style;
pub mod theme;
pub mod widgets;
