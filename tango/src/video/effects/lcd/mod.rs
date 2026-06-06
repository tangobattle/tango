use super::COMMON;
use crate::video::framebuffer::Effect;

pub const LCD: Effect = Effect {
    id: "lcd",
    name: "LCD",
    scale: 1,
    parts: &[COMMON, include_str!("lcd.wgsl")],
};
