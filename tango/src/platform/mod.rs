//! Host-machine glue — everything that talks to the OS or hardware,
//! and nothing that knows about tabs, saves, or netplay:
//!
//! * [`sdl_init`] + [`audio`] + [`gamepad`]: the SDL3 context and the
//!   two subsystems that ride it.
//! * [`input`] + [`input_capture`]: physical-input mapping for the
//!   emulator sessions and the capture flow that rebinds it.
//! * [`video`]: the wgpu framebuffer widget and its upscale effects.
//! * [`crash_log`]: the in-process half of native crash capture (the
//!   supervisor half lives in `main`).

pub mod audio;
pub mod crash_log;
pub mod gamepad;
pub mod input;
pub mod input_capture;
pub mod sdl_init;
pub mod video;
