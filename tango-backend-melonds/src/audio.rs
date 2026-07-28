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

    fn drain(&mut self, out: &mut [i16]) -> usize {
        let player = self.player.load(Ordering::Relaxed);
        self.link.lock().unwrap().console(player).read_audio(out)
    }
}

/// This console's audio, resampled — what a host holds.
pub fn pull(link: Arc<Mutex<crate::Link>>, player: Arc<AtomicUsize>) -> tango_match::Resampled<ConsoleAudio> {
    tango_match::Resampled::new(ConsoleAudio::new(link, player))
}
