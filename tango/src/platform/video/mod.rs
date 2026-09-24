//! Live emulator video presentation.
//!
//! The session's native frame — one GBA screen, or a DS's arranged pair —
//! is uploaded to a persistent GPU texture and drawn through a pluggable
//! WGSL [`framebuffer::Effect`] that does any upscaling (hqx/mmpx) on the
//! GPU.

pub mod effects;
pub mod framebuffer;
