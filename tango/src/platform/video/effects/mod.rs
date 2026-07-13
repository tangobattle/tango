//! The pluggable framebuffer effect registry. Each [`Effect`] is a GPU
//! upscaler (or the pass-through) defined as a named constant in a submodule
//! — `hqx::HQ2X`, `mmpx::MMPX`, etc. — built from the WGSL `.wgsl` files in
//! this directory. [`EFFECTS`] maps the `config.video_filter` key to each.

use crate::platform::video::framebuffer::Effect;

pub mod hqx;
pub mod lcd;
pub mod mmpx;

/// Shared infrastructure WGSL (vertex shader, bindings, `load`); prepended to
/// every effect.
pub(crate) const COMMON: &str = include_str!("common.wgsl");

/// Nearest pass-through — the "—" / no-filter default and the fallback for
/// unknown keys.
pub const PASSTHROUGH: Effect = Effect {
    id: "",
    name: "—",
    scale: 1,
    parts: &[COMMON, include_str!("passthrough.wgsl")],
};

/// The registry, in pick-list order. The `&str` is the `config.video_filter`
/// key (unchanged from the old CPU registry, so existing configs keep
/// working); the first entry is the canonical pass-through.
pub static EFFECTS: &[&Effect] = &[&PASSTHROUGH, &hqx::HQ2X, &hqx::HQ3X, &hqx::HQ4X, &mmpx::MMPX, &lcd::LCD];

/// Resolve a `config.video_filter` key to its effect. Unknown / empty keys
/// fall back to the pass-through (index 0).
pub fn effect_for(id: &str) -> &'static Effect {
    EFFECTS
        .iter()
        .find(|effect| effect.id == id)
        .cloned()
        .unwrap_or(&PASSTHROUGH)
}
