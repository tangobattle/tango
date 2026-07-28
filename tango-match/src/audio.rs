//! Getting a console's sound to a device.
//!
//! A console produces at its own rate and a device wants another, so
//! something has to resample — and a host also nudges the *claimed*
//! source rate to servo its output queue toward a target level, which
//! is how a session holds latency steady without pitch-shifting.
//!
//! None of that is per-console. A backend only has to say how fast its
//! console produces and hand over what it has ([`AudioDrain`]);
//! [`Resampled`] supplies the rest, so there is one resampler rather
//! than one per emulator.

/// Raw audio out of one console.
pub trait AudioDrain: Send {
    /// The rate this console produces at, in Hz.
    fn sample_rate(&self) -> f64;

    /// Take whatever the console has produced, as interleaved stereo,
    /// returning frames written. Taking it consumes it — which is what
    /// stops a session replaying audio it already played after a
    /// rollback.
    fn drain(&mut self, out: &mut [i16]) -> usize;

    /// How production scales when the host paces the simulation at
    /// `fps_target` — a throttled sim stretches playback by the same
    /// ratio instead of starving it, and a fast-forwarded one
    /// compresses. 1.0 means the console runs at its own rate.
    fn framerate_ratio(&self, _fps_target: f64) -> f64 {
        1.0
    }
}

/// Cross-thread pull of one console's audio, for a host's sound
/// callback.
pub trait AudioPull: Send {
    /// The rate the console truly produces at, in Hz.
    fn sample_rate(&self) -> f64;

    /// Pull whatever the console has produced, and report how much is
    /// now waiting to be resampled. The host's servo reads this to
    /// decide how hard to trim.
    fn source_available(&mut self) -> usize;

    /// Resample as if the console produced at `claimed_source_rate`,
    /// producing at `destination_rate`.
    ///
    /// Claiming a rate above the true one makes each output frame eat
    /// more input (draining the queue); below, less (refilling it).
    fn process(&mut self, claimed_source_rate: f64, destination_rate: f64);

    /// Frames ready for the device.
    fn available(&self) -> usize;

    /// Take up to `frames`, returning how many were written.
    fn read(&mut self, out: &mut [i16], frames: usize) -> usize;

    /// See [`AudioDrain::framerate_ratio`].
    fn framerate_ratio(&self, fps_target: f64) -> f64;

    /// Throw away the oldest `frames` of unresampled audio.
    ///
    /// A producer burst — a seek chase, a perspective swap, catching up
    /// after a device stall — is better skipped than carried as seconds
    /// of latency.
    fn discard_source(&mut self, frames: usize);
}

impl AudioPull for Box<dyn AudioPull> {
    fn sample_rate(&self) -> f64 {
        (**self).sample_rate()
    }

    fn source_available(&mut self) -> usize {
        (**self).source_available()
    }

    fn process(&mut self, claimed_source_rate: f64, destination_rate: f64) {
        (**self).process(claimed_source_rate, destination_rate)
    }

    fn available(&self) -> usize {
        (**self).available()
    }

    fn read(&mut self, out: &mut [i16], frames: usize) -> usize {
        (**self).read(out, frames)
    }

    fn framerate_ratio(&self, fps_target: f64) -> f64 {
        (**self).framerate_ratio(fps_target)
    }

    fn discard_source(&mut self, frames: usize) {
        (**self).discard_source(frames)
    }
}

/// How much console audio to hold before resampling. A 60 Hz frame is
/// under a thousand frames at any rate a handheld runs at; this is a few
/// frames of slack so a late callback does not starve.
const DRAIN_CHUNK: usize = 4096;

/// Cap on resampled audio held for the device, in frames — a third of a
/// second at 48 kHz, far more than any callback asks for and far less
/// than a fast-forward can pile up in a second.
const DEST_CAP: usize = 0x4000;

/// Any [`AudioDrain`], resampled — the shared half of the contract.
///
/// Linear interpolation over a fractional cursor: deliberately the
/// simplest thing that honours a claimed rate, since the cursor
/// advances by `claimed / destination` per output frame and the queue
/// therefore drifts exactly the way a servo intends.
pub struct Resampled<D> {
    drain: D,
    /// Pulled from the console, not yet resampled.
    source: Vec<i16>,
    /// Fractional position into `source`, in frames.
    cursor: f64,
    /// Resampled, waiting for the device.
    dest: Vec<i16>,
}

impl<D: AudioDrain> Resampled<D> {
    pub fn new(drain: D) -> Self {
        Resampled {
            drain,
            source: Vec::new(),
            cursor: 0.0,
            dest: Vec::new(),
        }
    }
}

impl<D: AudioDrain> AudioPull for Resampled<D> {
    fn sample_rate(&self) -> f64 {
        self.drain.sample_rate()
    }

    fn source_available(&mut self) -> usize {
        let mut scratch = vec![0i16; DRAIN_CHUNK * 2];
        let frames = self.drain.drain(&mut scratch);
        self.source.extend_from_slice(&scratch[..frames * 2]);
        (self.source.len() / 2).saturating_sub(self.cursor as usize)
    }

    fn process(&mut self, claimed_source_rate: f64, destination_rate: f64) {
        let step = claimed_source_rate / destination_rate;
        // A non-positive or non-finite step would never advance the
        // cursor, so the loop below would push forever.
        if !step.is_finite() || step <= 0.0 {
            return;
        }
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
        // interpolates from. The loop's last step can carry the cursor
        // past the end of the queue (any step over 1 gets there — the
        // fast-forward fold divides the destination rate, so a 65536 Hz
        // cart at 4x steps ~5.5 frames at a time); the drain must not
        // follow it out of bounds. The overshoot stays in the cursor as
        // a debt the next batch of source pays off.
        let consumed = (self.cursor as usize).min(frames);
        if consumed > 0 {
            self.source.drain(..consumed * 2);
            self.cursor -= consumed as f64;
        }
        // Bound what is waiting for the device, dropping the oldest.
        //
        // Production and consumption are not always balanced: a
        // fast-forwarded session claims a destination rate above the
        // device's, so this produces several frames for every one the
        // callback takes and the surplus is real. Discarding it keeps
        // latency bounded, which is exactly what the fixed-size ring
        // this replaced did by overwriting.
        let over = self.dest.len().saturating_sub(DEST_CAP * 2);
        if over > 0 {
            self.dest.drain(..over);
        }
    }

    fn available(&self) -> usize {
        self.dest.len() / 2
    }

    fn framerate_ratio(&self, fps_target: f64) -> f64 {
        self.drain.framerate_ratio(fps_target)
    }

    fn discard_source(&mut self, frames: usize) {
        let frames = frames.min(self.source.len() / 2);
        self.source.drain(..frames * 2);
    }

    fn read(&mut self, out: &mut [i16], frames: usize) -> usize {
        let frames = frames.min(self.dest.len() / 2).min(out.len() / 2);
        out[..frames * 2].copy_from_slice(&self.dest[..frames * 2]);
        self.dest.drain(..frames * 2);
        frames
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A console that always has audio ready, at a GBA-ish rate.
    struct Endless;

    impl AudioDrain for Endless {
        fn sample_rate(&self) -> f64 {
            32768.0
        }

        fn drain(&mut self, out: &mut [i16]) -> usize {
            out.fill(1);
            out.len() / 2
        }
    }

    /// Fast-forward claims a destination rate well above the device's,
    /// so each fill resamples up. Without a bound the surplus is kept
    /// forever and the process grows until it dies — which is what
    /// holding fast-forward used to do.
    #[test]
    fn fast_forward_does_not_grow_the_queue_without_end() {
        let mut pull = Resampled::new(Endless);
        for _ in 0..200 {
            pull.source_available();
            // 4x fast-forward: 48 kHz device, 192 kHz claimed.
            pull.process(32768.0, 192_000.0);
            let mut out = [0i16; 1024];
            pull.read(&mut out, 512);
        }
        assert!(
            pull.available() <= DEST_CAP,
            "resampled queue grew to {} frames",
            pull.available()
        );
    }

    /// A console with a fixed batch of audio and nothing more, at BN4+'s
    /// doubled rate.
    struct Burst(usize);

    impl AudioDrain for Burst {
        fn sample_rate(&self) -> f64 {
            65536.0
        }

        fn drain(&mut self, out: &mut [i16]) -> usize {
            let frames = self.0.min(out.len() / 2);
            self.0 -= frames;
            out[..frames * 2].fill(1);
            frames
        }
    }

    /// The resample loop's last step can carry the cursor past the end
    /// of the source queue whenever the step exceeds 1 — under the
    /// fast-forward fold a 65536 Hz cart against a 48 kHz device at 4x
    /// steps ~5.5 source frames per output frame. The drain after the
    /// loop must not follow the cursor out of bounds: with 8 frames
    /// queued the cursor lands past 10, and draining to there panicked
    /// in the sound callback (which aborts the process).
    #[test]
    fn cursor_overshoot_does_not_drain_past_the_source_end() {
        let mut pull = Resampled::new(Burst(8));
        pull.source_available();
        // The first fill after speed-up engages the fold before the
        // stretcher does: destination = 48000 × (59.7275 / 240).
        pull.process(65536.0, 11945.5);
        assert_eq!(pull.available(), 2);
    }

    /// A zero or negative step can never advance the cursor, so the
    /// resample loop would push output forever.
    #[test]
    fn a_degenerate_rate_produces_nothing_rather_than_hanging() {
        let mut pull = Resampled::new(Endless);
        pull.source_available();
        pull.process(0.0, 48_000.0);
        assert_eq!(pull.available(), 0);
        pull.process(32768.0, 0.0);
        assert_eq!(pull.available(), 0);
    }
}
