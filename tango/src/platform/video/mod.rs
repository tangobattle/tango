//! Live emulator video presentation.
//!
//! The native 240×160 frame is uploaded to a persistent GPU texture and drawn
//! through a pluggable WGSL [`framebuffer::Effect`] that does any upscaling
//! (hqx/mmpx) on the GPU. The CPU upscalers that used to run on the UI thread
//! each vblank are gone — the workspace `hqx`/`mmpx` crates remain in-tree but
//! are no longer used here.

pub mod effects;
pub mod framebuffer;
