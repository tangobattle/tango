//! DS audio: a drain, and nothing else.
//!
//! Worth knowing about the buffer behind it: melonDS's SPU sizes its
//! output ring from the output rate, which at the 48 kHz the shim fixes
//! is 2048 frames — about 43 ms, less than a session queues. It also
//! answers an overflow by advancing its own read cursor, destroying the
//! oldest audio rather than refusing the newest. So a host must drain
//! this promptly and hold its backlog itself; there is no leaving one
//! here.
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

    /// One lock for all of it — the SPU sits behind the same mutex the
    /// simulation ticks under, and this runs on the sound callback.
    fn drain(&mut self, out: &mut [i16]) -> tango_match::Drained {
        let player = self.player.load(Ordering::Relaxed);
        let mut link = self.link.lock().unwrap();
        let console = link.console(player);
        let written = console.read_audio(out);
        tango_match::Drained {
            written,
            queued: console.audio_queued(),
        }
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
