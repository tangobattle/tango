use super::COMMON;
use crate::platform::video::framebuffer::{Effect, WgslRenderer};

static RENDERER: WgslRenderer = WgslRenderer::new(&[COMMON, include_str!("lcd.wgsl")]);
pub const LCD: Effect = Effect::new("lcd", "LCD", 1, &RENDERER);
