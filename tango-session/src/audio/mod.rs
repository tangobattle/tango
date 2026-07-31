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

/// A session's audio before its pair exists.
///
/// A host binds its output stream when the session is built, but the
/// pairs that produce sound are booted on the drive loop — a priming
/// walk later, which is seconds on a DS-class game. This stands in
/// until then, reporting silence, and starts delegating the moment the
/// boot hands over a real drain. Both link-battle session kinds (live
/// PvP and replay playback) come up this way.
#[derive(Clone, Default)]
pub struct DeferredDrain(std::sync::Arc<std::sync::Mutex<Option<Box<dyn tango_match::AudioDrain>>>>);

impl DeferredDrain {
    /// Hand over the real drain — the boot's last step, after which
    /// every call below delegates.
    pub fn set(&self, drain: Box<dyn tango_match::AudioDrain>) {
        *self.0.lock().unwrap() = Some(drain);
    }

    fn with<R>(&self, fallback: R, f: impl FnOnce(&mut Box<dyn tango_match::AudioDrain>) -> R) -> R {
        match self.0.lock().unwrap().as_mut() {
            Some(drain) => f(drain),
            None => fallback,
        }
    }
}

impl tango_match::AudioDrain for DeferredDrain {
    fn sample_rate(&self) -> f64 {
        // The GBA's own rate, which is what every game a session runs
        // produces at and only stands in while the pair is booting.
        self.with(32768.0, |d| d.sample_rate())
    }

    fn drain(&mut self, out: &mut [i16]) -> tango_match::Drained {
        self.with(tango_match::Drained::default(), |d| d.drain(out))
    }

    fn framerate_ratio(&self, fps_target: f64) -> f64 {
        self.with(1.0, |d| d.framerate_ratio(fps_target))
    }
}
