//! DS audio: a drain, and nothing else.
//!
//! The SPU mixes at a fixed rate and hands out interleaved stereo, so
//! all this says is "here is what the console produced". Resampling is
//! [`tango_match::Resampler`]'s job.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tango_match::AudioDrain;

/// Frames melonDS's SPU output ring holds.
///
/// Not a knob — the SPU sizes it itself, from the output rate: enough
/// for one frame of audio (`ceil(rate / 60)`) rounded up to a power of
/// two, then doubled. At the 48 kHz the shim fixes, that is 2048 frames,
/// or about 43 ms.
///
/// Worth stating because it is small. A host that leaves its audio
/// backlog in the console — a GBA core will happily hold a second of it
/// — would here be asking for several times what the ring holds, and
/// this ring answers an overflow by advancing its own read cursor: it
/// destroys the oldest audio rather than refusing the newest. The
/// backlog has to live on the host's side of this one.
const SPU_BUFFER_FRAMES: usize = 2048;

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
            capacity: SPU_BUFFER_FRAMES,
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
