//! MMPX 2x GPU magnifier: the shared [`COMMON`] prelude + the hand-written
//! `mmpx.wgsl` rule cascade (which defines its own `luma`/equality helpers).

use super::COMMON;
use crate::platform::video::framebuffer::{Effect, WgslRenderer};

static RENDERER: WgslRenderer = WgslRenderer::new(&[COMMON, include_str!("mmpx.wgsl")]);
pub const MMPX: Effect = Effect::new("mmpx", "mmpx", 2, &RENDERER);
