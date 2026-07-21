pub mod webgl;

pub const SCREEN_WIDTH: usize = 240;
pub const SCREEN_HEIGHT: usize = 160;
/// mgba is built 16-bit: one little-endian BGR555 halfword per pixel.
pub const SCREEN_BYTES: usize = SCREEN_WIDTH * SCREEN_HEIGHT * 2;
