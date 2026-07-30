//! Audio: what a host's output stream pulls from, and the thing that
//! pulls it off a running emulator.
//!
//! [`Stream`] is the seam — a host binds one and asks it for frames —
//! and [`CoreStream`] is the only implementation a session needs: it
//! takes a backend's raw [`AudioDrain`](tango_match::AudioDrain), owns
//! the resampler over it, and runs the dynamic rate control that decides
//! how fast to play (see [`core_stream`] for why that's the whole
//! design).

pub mod core_stream;
mod resampler;

pub use core_stream::{CoreStream, Intake};
pub use resampler::Resampler;

pub const NUM_CHANNELS: usize = 2;
pub const SAMPLES: usize = 512;

pub trait Stream {
    fn fill(&mut self, buf: &mut [[i16; NUM_CHANNELS]]) -> usize;
}
