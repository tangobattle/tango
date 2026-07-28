//! [`AudioPull`] over a live GBA pair.
//!
//! mgba's resampler already matches its own cores' buffers exactly —
//! including consuming from them, which is what keeps a session from
//! replaying audio it has already played after a rollback — so the
//! implementation is a thin hold of that plus the destination buffer.

use tango_match::AudioPull;

/// One player's audio off a running pair.
pub struct PairAudio {
    pair: mgba_rollback::session::LinkHandle,
    /// Re-read every fill: a replay's perspective swap flips it, while
    /// a PvP session pins it to the local player.
    player: Box<dyn Fn() -> usize + Send>,
    resampler: mgba::audio::AudioResampler,
    dest: mgba::audio::OwnedAudioBuffer,
}

impl PairAudio {
    /// `capacity` is in frames at the device rate.
    pub fn new(
        pair: mgba_rollback::session::LinkHandle,
        player: Box<dyn Fn() -> usize + Send>,
        capacity: usize,
    ) -> Self {
        PairAudio {
            pair,
            player,
            resampler: mgba::audio::AudioResampler::new(),
            dest: mgba::audio::OwnedAudioBuffer::new(capacity, 2),
        }
    }
}

impl AudioPull for PairAudio {
    fn sample_rate(&self) -> f64 {
        let player = (self.player)();
        self.pair
            .with_link(|pair| pair.core_mut(player).audio_sample_rate() as f64)
    }

    fn source_available(&self) -> usize {
        let player = (self.player)();
        self.pair
            .with_link(|pair| pair.core_mut(player).audio_buffer().available())
    }

    fn process(&mut self, claimed_source_rate: f64, destination_rate: f64) {
        let player = (self.player)();
        let resampler = &mut self.resampler;
        let dest = &mut self.dest;
        self.pair.with_link(|pair| {
            // Consuming: samples handed over are gone from the core's
            // buffer.
            resampler.set_source(pair.core_mut(player).audio_buffer(), claimed_source_rate, true);
            resampler.set_destination(dest, destination_rate);
            resampler.process();
        });
    }

    fn available(&self) -> usize {
        self.dest.available()
    }

    fn read(&mut self, out: &mut [i16], frames: usize) -> usize {
        self.dest.read(out, frames)
    }
}
