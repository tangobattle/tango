//! Audio: what a host's output stream pulls from, and the thing that
//! pulls it off a running emulator.
//!
//! [`Source`] is the seam — a host's output device pulls frames from
//! one — and [`Stream`] is the only implementation a session needs: it
//! takes a running console's raw [`Drain`], owns the resampler over it,
//! and runs the dynamic rate control that decides how fast to play (see
//! [`stream`] for why that's the whole design).
//!
//! All of it lives above the engine seam, because none of it is an
//! emulator's business: a backend answers for one console's rate and
//! its sample queue ([`Side`](tango_match::Side)) and nothing else, and
//! [`side_drain`] turns that into something a host can pull on.

mod drain;
mod resampler;
pub mod stream;

pub use drain::{side_drain, Drain};
pub use resampler::Resampler;
pub use stream::{Intake, Stream};

pub const NUM_CHANNELS: usize = 2;
pub const SAMPLES: usize = 512;

/// Whatever a host's output device pulls its frames from. A session
/// hands over a [`Stream`]; a host may put its own between (the
/// desktop's late-binding mux, so the device outlives any one session).
pub trait Source {
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
pub struct DeferredDrain(std::sync::Arc<std::sync::Mutex<Option<Box<dyn Drain>>>>);

impl DeferredDrain {
    /// Hand over the real drain — the boot's last step, after which
    /// every call below delegates.
    pub fn set(&self, drain: Box<dyn Drain>) {
        *self.0.lock().unwrap() = Some(drain);
    }

    fn with<R>(&self, fallback: R, f: impl FnOnce(&mut Box<dyn Drain>) -> R) -> R {
        match self.0.lock().unwrap().as_mut() {
            Some(drain) => f(drain),
            None => fallback,
        }
    }
}

impl Drain for DeferredDrain {
    fn sample_rate(&self) -> f64 {
        // The GBA's own rate, which is what every game a session runs
        // produces at and only stands in while the pair is booting.
        self.with(32768.0, |d| d.sample_rate())
    }

    fn drain(&mut self, out: &mut [i16]) -> Option<usize> {
        // `Some(0)` rather than `None` while the pair boots: there is
        // genuinely nothing yet, which is a fact, where `None` would
        // leave a caller holding a level from a console that does not
        // exist.
        self.with(Some(0), |d| d.drain(out))
    }
}
