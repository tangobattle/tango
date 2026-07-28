//! Pulling audio out of a GBA pair.
//!
//! A console produces at its own rate and a sound device wants another,
//! so somebody has to resample. mgba ships a resampler that already
//! matches its cores' output exactly, so this is where it belongs —
//! the seam only says how to get samples out of a link
//! ([`Backend::drain_audio`](tango_match::Backend::drain_audio)), not
//! how to rate-convert them.

use tango_match::Backend;

/// Resamples one console's output to a device rate.
pub struct Resampler {
    inner: mgba::audio::AudioResampler,
    dest: mgba::audio::OwnedAudioBuffer,
    player: usize,
}

impl Resampler {
    /// `capacity` is in stereo frames at the device rate.
    pub fn new(player: usize, capacity: usize) -> Self {
        Resampler {
            inner: mgba::audio::AudioResampler::new(),
            dest: mgba::audio::OwnedAudioBuffer::new(capacity, 2),
            player,
        }
    }

    pub fn player(&self) -> usize {
        self.player
    }

    /// Convert whatever the console has produced to `device_rate` and
    /// write it into `out` as interleaved stereo, returning the frames
    /// written.
    pub fn resample(&mut self, link: &mut mgba_rollback::Link, device_rate: f64, out: &mut [i16]) -> usize {
        let source_rate = crate::Mgba::sample_rate(link);
        // `consume` is true: samples handed to the resampler are gone
        // from the core's buffer, which is what keeps a live session
        // from replaying stale audio after a rollback.
        self.inner
            .set_source(link.core_mut(self.player).audio_buffer(), source_rate, true);
        self.inner.set_destination(&mut self.dest, device_rate);
        self.inner.process();
        let frames = (out.len() / 2).min(self.dest.available());
        self.dest.read(out, frames)
    }
}
