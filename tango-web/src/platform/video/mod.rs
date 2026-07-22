#[cfg(not(target_arch = "wasm32"))]
pub mod native;
#[cfg(target_arch = "wasm32")]
pub mod webgl;

pub const SCREEN_WIDTH: usize = 240;
pub const SCREEN_HEIGHT: usize = 160;
/// mgba is built 16-bit: one little-endian BGR555 halfword per pixel.
pub const SCREEN_BYTES: usize = SCREEN_WIDTH * SCREEN_HEIGHT * 2;

/// The video-filter registry, in the desktop's pick-list order:
/// `config.video_filter` key → display name. Keys match the desktop's
/// so configs mean the same thing. Both backends implement the full
/// set (the web backend via naga-transpiled GLSL, the native backend
/// via the desktop's WGSL directly).
pub static FILTERS: &[(&str, &str)] = &[
    ("", "—"),
    ("hq2x", "hq2x"),
    ("hq3x", "hq3x"),
    ("hq4x", "hq4x"),
    ("mmpx", "mmpx"),
    ("lcd", "LCD"),
];
