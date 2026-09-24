//! Host-machine glue — everything that talks to the OS or hardware,
//! and nothing that knows about tabs, saves, or netplay:
//!
//! * [`audio`]: CPAL audio output and session-stream routing.
//! * [`input`] + [`input_capture`]: physical-input mapping for the
//!   emulator sessions and the capture flow that rebinds it. Gamepad
//!   input itself comes from the standalone [`gamepad_facade`] crate.
//! * [`video`]: the wgpu framebuffer widget and its upscale effects.
//! * [`crash`]: native crash capture — the supervisor process and the
//!   hooks it installs in the child UI.

pub mod audio;
pub mod crash;
pub mod input;
pub mod input_capture;
pub mod video;
