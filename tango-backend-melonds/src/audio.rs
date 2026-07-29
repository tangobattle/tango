//! DS audio: a drain, and nothing else.
//!
//! The SPU mixes at a fixed rate and hands out interleaved stereo, so
//! all this says is "here is what the console produced". Resampling is
//! [`tango_match::Resampled`]'s job.

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

    fn queued(&mut self) -> usize {
        let player = self.player.load(Ordering::Relaxed);
        self.link.lock().unwrap().console(player).audio_queued()
    }

    fn drain(&mut self, out: &mut [i16]) -> usize {
        let player = self.player.load(Ordering::Relaxed);
        self.link.lock().unwrap().console(player).read_audio(out)
    }
}

/// This console's audio, resampled — what a host holds.
pub fn pull(link: Arc<Mutex<crate::Link>>, player: Arc<AtomicUsize>) -> tango_match::Resampled<ConsoleAudio> {
    tango_match::Resampled::new(ConsoleAudio::new(link, player))
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
