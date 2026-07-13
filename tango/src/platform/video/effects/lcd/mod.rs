use super::COMMON;
use crate::platform::video::framebuffer::Effect;

pub const LCD: Effect = Effect {
    id: "lcd",
    name: "LCD",
    scale: 1,
    parts: &[COMMON, include_str!("lcd.wgsl")],
};
