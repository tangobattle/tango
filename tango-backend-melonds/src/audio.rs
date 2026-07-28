//! [`AudioPull`] over a live DS pair.
//!
//! melonDS hands out interleaved stereo at a fixed rate and ships no
//! resampler, so this supplies one. It is deliberately the simplest
//! thing that honours the contract: linear interpolation over a
//! fractional read cursor, which is what lets a host claim a source
//! rate slightly off the true one and have the queue drift the way its
//! servo intends.

use std::sync::{Arc, Mutex};

use tango_match::{AudioPull, Backend};

/// How much console audio to hold before resampling. A frame at 60 Hz
/// is ~547 samples; this is a few frames of slack so a late callback
/// does not starve.
const SOURCE_CAPACITY: usize = 4096;

/// One player's audio off a running pair.
pub struct PairAudio {
    link: Arc<Mutex<crate::Link>>,
    player: usize,
    /// Interleaved stereo drained from the console, not yet resampled.
    source: Vec<i16>,
    /// Fractional position into `source`, in frames.
    cursor: f64,
    /// Resampled output waiting for the device.
    dest: Vec<i16>,
}

impl PairAudio {
    pub fn new(link: Arc<Mutex<crate::Link>>, player: usize) -> Self {
        PairAudio {
            link,
            player,
            source: Vec::with_capacity(SOURCE_CAPACITY * 2),
            cursor: 0.0,
            dest: Vec::new(),
        }
    }

    /// Pull whatever the console has produced since last time.
    fn refill(&mut self) {
        let mut scratch = [0i16; SOURCE_CAPACITY * 2];
        let frames = {
            let mut link = self.link.lock().unwrap();
            <crate::MelonDs as Backend>::drain_audio(&mut link, self.player, &mut scratch)
        };
        self.source.extend_from_slice(&scratch[..frames * 2]);
    }
}

impl AudioPull for PairAudio {
    fn sample_rate(&self) -> f64 {
        crate::SAMPLE_RATE
    }

    fn source_available(&self) -> usize {
        self.source.len() / 2 - self.cursor as usize
    }

    fn process(&mut self, claimed_source_rate: f64, destination_rate: f64) {
        self.refill();
        if destination_rate <= 0.0 {
            return;
        }
        // Frames of source consumed per frame of output. Claiming a
        // faster source than the console really is makes each output
        // frame eat more, which drains the queue — the servo's lever.
        let step = claimed_source_rate / destination_rate;
        let frames = self.source.len() / 2;
        while (self.cursor as usize) + 1 < frames {
            let i = self.cursor as usize;
            let frac = self.cursor - i as f64;
            for channel in 0..2 {
                let a = self.source[i * 2 + channel] as f64;
                let b = self.source[(i + 1) * 2 + channel] as f64;
                self.dest.push((a + (b - a) * frac) as i16);
            }
            self.cursor += step;
        }
        // Drop what the cursor has passed, keeping the frame it still
        // interpolates from.
        let consumed = self.cursor as usize;
        if consumed > 0 {
            self.source.drain(..consumed * 2);
            self.cursor -= consumed as f64;
        }
    }

    fn available(&self) -> usize {
        self.dest.len() / 2
    }

    fn read(&mut self, out: &mut [i16], frames: usize) -> usize {
        let frames = frames.min(self.dest.len() / 2).min(out.len() / 2);
        out[..frames * 2].copy_from_slice(&self.dest[..frames * 2]);
        self.dest.drain(..frames * 2);
        frames
    }
}
