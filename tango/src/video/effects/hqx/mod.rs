//! hqx GPU upscalers. Each effect is the shared [`super::COMMON`] prelude +
//! [`COMMON`] (the YUV metric and interpolation rules) + the generated
//! per-scale table (`hq{2,3,4}x.wgsl` in this directory).

use crate::video::framebuffer::Effect;

/// hqx-family prelude (`yuv_diff`/`diff`, `interp1..10`); pulled in only by
/// the hqx effects, between [`COMMON`] and the generated table.
const COMMON: &str = include_str!("common.wgsl");

pub const HQ2X: Effect = Effect {
    id: "hq2x",
    name: "hq2x",
    scale: 2,
    parts: &[super::COMMON, COMMON, include_str!("hq2x.wgsl")],
};
pub const HQ3X: Effect = Effect {
    id: "hq3x",
    name: "hq3x",
    scale: 3,
    parts: &[super::COMMON, COMMON, include_str!("hq3x.wgsl")],
};
pub const HQ4X: Effect = Effect {
    id: "hq4x",
    name: "hq4x",
    scale: 4,
    parts: &[super::COMMON, COMMON, include_str!("hq4x.wgsl")],
};
