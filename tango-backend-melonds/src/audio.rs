//! DS audio: a drain, and nothing else.
//!
//! Taken from the link rather than straight off the SPU. The link
//! empties each console's SPU every tick into a buffer of its own,
//! because the SPU's ring cannot serve as one: a savestate does not
//! cover it, so a rollback cannot take back what it speculated there,
//! and at ~43 ms it overflows within a couple of frames of a
//! re-simulation appending a span twice — destroying its own oldest
//! audio to make room. What arrives here is already revocable and
//! already deduplicated.
//!
//! The SPU mixes at a fixed rate and hands out interleaved stereo, so
//! all this says is "here is what the console produced". Resampling is
//! [`tango_match::Resampler`]'s job.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tango_match::AudioDrain;


/// One player's console on a running pair.
pub struct ConsoleAudio {
    link: Arc<Mutex<crate::Link>>,
    /// Read per fill: a host that moves the sound between seats does so
    /// without the resampler above being rebuilt under it.
    player: Arc<AtomicUsize>,
}

impl ConsoleAudio {
    pub fn new(link: Arc<Mutex<crate::Link>>, player: Arc<AtomicUsize>) -> Self {
        ConsoleAudio { link, player }
    }
}

impl AudioDrain for ConsoleAudio {
    fn sample_rate(&self) -> f64 {
        crate::SAMPLE_RATE
    }

    fn framerate_ratio(&self, fps_target: f64) -> f64 {
        crate::framerate_ratio(fps_target)
    }

    /// One lock for both facts — the link sits behind the same mutex the
    /// simulation ticks under, and this runs on the sound callback.
    fn drain(&mut self, out: &mut [i16]) -> tango_match::Drained {
        let player = self.player.load(Ordering::Relaxed);
        let (written, queued) = self.link.lock().unwrap().take_audio(player, out);
        tango_match::Drained { written, queued }
    }
}

/// This console's audio, as the raw source a host resamples.
pub fn pull(link: Arc<Mutex<crate::Link>>, player: Arc<AtomicUsize>) -> ConsoleAudio {
    ConsoleAudio::new(link, player)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stream reads this as "how long a second of production
    /// lasts" and scales its resampler's destination rate by it, so a
    /// fast-forward has to push it *below* 1.0. An inverted ratio still produces
    /// sound — just slowed-down sound where sped-up was wanted — which
    /// is exactly why the direction is worth pinning.
    #[test]
    fn the_ratio_matches_mgbas_convention() {
        let ratio = crate::framerate_ratio;
        assert!(ratio(crate::FPS * 3.0) < 1.0, "fast-forward must compress");
        assert!(ratio(crate::FPS / 2.0) > 1.0, "throttling must stretch");
        assert!((ratio(crate::FPS) - 1.0).abs() < 1e-9, "native speed is 1.0");
    }
}
