//! Host-machine glue — everything that talks to the OS or hardware,
//! and nothing that knows about tabs, saves, or netplay:
//!
//! * [`audio`]: CPAL audio output and session-stream routing.
//! * [`input`] + [`input_capture`]: physical-input mapping for the
//!   emulator sessions and the capture flow that rebinds it. Gamepad
//!   input itself comes from the standalone [`sdl3_gamepad`] crate.
//! * [`video`]: the wgpu framebuffer widget and its upscale effects.
//! * [`crash_log`]: the in-process half of native crash capture (the
//!   supervisor half lives in `main`).

pub mod audio;
pub mod crash_log;
pub mod input;
pub mod input_capture;
pub mod video;
