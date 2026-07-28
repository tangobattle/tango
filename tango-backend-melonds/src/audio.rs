//! DS audio: a drain, and nothing else.
//!
//! The SPU mixes at a fixed rate and hands out interleaved stereo, so
//! all this says is "here is what the console produced". Resampling is
//! [`tango_match::Resampled`]'s job.

use std::sync::{Arc, Mutex};

use tango_match::AudioDrain;

/// One player's console on a running pair.
pub struct ConsoleAudio {
    link: Arc<Mutex<crate::Link>>,
    player: usize,
}

impl ConsoleAudio {
    pub fn new(link: Arc<Mutex<crate::Link>>, player: usize) -> Self {
        ConsoleAudio { link, player }
    }
}

impl AudioDrain for ConsoleAudio {
    fn sample_rate(&self) -> f64 {
        crate::SAMPLE_RATE
    }

    fn drain(&mut self, out: &mut [i16]) -> usize {
        self.link.lock().unwrap().console(self.player).read_audio(out)
    }
}

/// This console's audio, resampled — what a host holds.
pub fn pull(link: Arc<Mutex<crate::Link>>, player: usize) -> tango_match::Resampled<ConsoleAudio> {
    tango_match::Resampled::new(ConsoleAudio::new(link, player))
}
