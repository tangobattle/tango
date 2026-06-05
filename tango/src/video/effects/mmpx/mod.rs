//! MMPX 2x GPU magnifier: the shared [`COMMON`] prelude + the hand-written
//! `mmpx.wgsl` rule cascade (which defines its own `luma`/equality helpers).

use super::COMMON;
use crate::video::framebuffer::Effect;

pub const MMPX: Effect = Effect {
    id: "mmpx",
    name: "mmpx",
    scale: 2,
    parts: &[COMMON, include_str!("mmpx.wgsl")],
};
