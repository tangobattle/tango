//! GBA audio: a drain, and nothing else.
//!
//! Resampling is [`tango_match::Resampled`]'s job — one implementation
//! for every console rather than mgba's for this one and something else
//! for the next.

use tango_match::AudioDrain;

/// One player's core on a running pair.
pub struct ConsoleAudio {
    pair: mgba_rollback::session::LinkHandle,
    /// Re-read every fill: a replay's perspective swap flips it, while
    /// a PvP session pins it to the local player.
    player: Box<dyn Fn() -> usize + Send>,
}

impl ConsoleAudio {
    pub fn new(pair: mgba_rollback::session::LinkHandle, player: Box<dyn Fn() -> usize + Send>) -> Self {
        ConsoleAudio { pair, player }
    }
}

impl AudioDrain for ConsoleAudio {
    fn sample_rate(&self) -> f64 {
        let player = (self.player)();
        self.pair
            .with_link(|pair| pair.core_mut(player).audio_sample_rate() as f64)
    }

    fn framerate_ratio(&self, fps_target: f64) -> f64 {
        let player = (self.player)();
        self.pair
            .with_link(|pair| pair.core_mut(player).calculate_framerate_ratio(fps_target))
    }

    /// One lock for both: `with_link` takes the same mutex the engine
    /// ticks under, and this runs on the sound callback.
    fn drain(&mut self, out: &mut [i16]) -> tango_match::Drained {
        let player = (self.player)();
        self.pair.with_link(|pair| {
            let buffer = pair.core_mut(player).audio_buffer();
            // `out` holds interleaved samples, so it fits half as many
            // frames. Reading consumes, which is what stops a session
            // replaying audio it already played after a rollback.
            let frames = (out.len() / 2).min(buffer.available());
            let written = buffer.read(out, frames);
            // What stays here stays revocable: this is the buffer the
            // rollback engine takes speculated audio back out of.
            tango_match::Drained {
                written,
                queued: buffer.available(),
            }
        })
    }
}

/// This console's audio, resampled — what a host holds.
pub fn pull(
    pair: mgba_rollback::session::LinkHandle,
    player: Box<dyn Fn() -> usize + Send>,
) -> tango_match::Resampled<ConsoleAudio> {
    tango_match::Resampled::new(ConsoleAudio::new(pair, player))
}
