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

    /// Frames the console is holding.
    ///
    /// This is where a session's audio backlog lives, and answering it
    /// is what lets a host leave the backlog there and take only what it
    /// is about to play. That is not a nicety: a rollback revokes
    /// speculated audio out of this buffer, so anything pulled early is
    /// beyond its reach — the mispredicted span plays anyway, and its
    /// corrected re-simulation has to be swallowed to avoid an echo.
    fn queued(&mut self) -> usize;

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

    /// How much console audio is waiting to be resampled, counting what
    /// the console still holds. This is a session's audio backlog — its
    /// latency, and what a host's rate control steers.
    fn source_available(&mut self) -> usize;

    /// Top the resampled queue up to `frames`, as if the console
    /// produced at `claimed_source_rate` and the device wanted
    /// `destination_rate`.
    ///
    /// Claiming a rate above the true one makes each output frame eat
    /// more input (draining the queue); below, less (refilling it).
    ///
    /// `frames` is what the caller is about to play, and resampling
    /// stops there rather than converting everything queued: the
    /// unresampled side is the buffer a host's servo measures and
    /// steers, so audio held past this point would be backlog nothing
    /// regulates.
    fn process(&mut self, claimed_source_rate: f64, destination_rate: f64, frames: usize);

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

    fn process(&mut self, claimed_source_rate: f64, destination_rate: f64, frames: usize) {
        (**self).process(claimed_source_rate, destination_rate, frames)
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

/// Any [`AudioDrain`], resampled — the shared half of the contract.
///
/// Linear interpolation over a fractional cursor: deliberately the
/// simplest thing that honours a claimed rate, since the cursor
/// advances by `claimed / destination` per output frame and the queue
/// therefore drifts exactly the way a servo intends.
pub struct Resampled<D> {
    drain: D,
    /// Pulled from the console, not yet resampled — only ever what the
    /// current fill needs. The backlog stays in the console's own buffer
    /// ([`AudioDrain::queued`]), where a rollback's revocation can still
    /// reach it.
    source: Vec<i16>,
    /// Fractional position into `source`, in frames.
    cursor: f64,
    /// Resampled, waiting for the device — a hand-off buffer, never
    /// more than the last [`process`](Resampled::process) was asked for.
    dest: Vec<i16>,
    /// Reusable landing buffer for [`AudioDrain::drain`]. A sound
    /// callback must not allocate.
    scratch: Vec<i16>,
}

impl<D: AudioDrain> Resampled<D> {
    pub fn new(drain: D) -> Self {
        Resampled {
            drain,
            source: Vec::new(),
            cursor: 0.0,
            dest: Vec::new(),
            scratch: Vec::new(),
        }
    }

    /// Frames still to be resampled out of `source`.
    fn staged(&self) -> usize {
        (self.source.len() / 2).saturating_sub(self.cursor as usize)
    }

    /// Pull up to `frames` more out of the console into `source`,
    /// returning how many landed.
    fn pull(&mut self, frames: usize) -> usize {
        let frames = frames.min(DRAIN_CHUNK);
        if frames == 0 {
            return 0;
        }
        self.scratch.resize(frames * 2, 0);
        let got = self.drain.drain(&mut self.scratch);
        self.source.extend_from_slice(&self.scratch[..got * 2]);
        got
    }
}

impl<D: AudioDrain> AudioPull for Resampled<D> {
    fn sample_rate(&self) -> f64 {
        self.drain.sample_rate()
    }

    fn source_available(&mut self) -> usize {
        // Counted, not pulled: the backlog stays in the console's own
        // buffer, where a rollback's revocation can still reach it.
        self.staged() + self.drain.queued()
    }

    fn process(&mut self, claimed_source_rate: f64, destination_rate: f64, frames: usize) {
        let step = claimed_source_rate / destination_rate;
        // A non-positive or non-finite step would never advance the
        // cursor, so the loop below would push forever.
        if !step.is_finite() || step <= 0.0 {
            return;
        }
        // Pull in exactly what this fill needs and no more, leaving any
        // backlog in the console's buffer. The `+ 2` covers the frame
        // each output interpolates *towards* plus the cursor's fraction.
        let want = frames.saturating_sub(self.dest.len() / 2);
        let need = (self.cursor + want as f64 * step).ceil().max(0.0) as usize + 2;
        if need > self.source.len() / 2 {
            self.pull(need - self.source.len() / 2);
        }

        let available = self.source.len() / 2;
        // Stop at what the caller is about to play. Converting the whole
        // source queue instead would hand the backlog downstream of the
        // servo — which reads `source_available` — so the level it holds
        // would be whatever one fill happens to produce, its trim would
        // sit pinned at full authority, and the surplus would pile up
        // here as latency until something had to shed it in whole spans.
        // A rollback catch-up, which lands several ticks of production
        // between two fills, shed the biggest spans of all.
        for _ in 0..want {
            let i = self.cursor as usize;
            if i + 1 >= available {
                break;
            }
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
        let consumed = (self.cursor as usize).min(available);
        if consumed > 0 {
            self.source.drain(..consumed * 2);
            self.cursor -= consumed as f64;
        }
    }

    fn available(&self) -> usize {
        self.dest.len() / 2
    }

    fn framerate_ratio(&self, fps_target: f64) -> f64 {
        self.drain.framerate_ratio(fps_target)
    }

    fn discard_source(&mut self, frames: usize) {
        // Staged first, then through to the console — the backlog being
        // shed mostly sits there now.
        let staged = frames.min(self.source.len() / 2);
        self.source.drain(..staged * 2);
        // The cursor indexed into what was just dropped; the point of
        // discarding is to jump forward, so it restarts at the head.
        self.cursor = 0.0;
        let mut rest = frames - staged;
        while rest > 0 {
            self.scratch.resize(rest.min(DRAIN_CHUNK) * 2, 0);
            let got = self.drain.drain(&mut self.scratch);
            if got == 0 {
                break;
            }
            rest -= got;
        }
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

        fn queued(&mut self) -> usize {
            DRAIN_CHUNK
        }

        fn drain(&mut self, out: &mut [i16]) -> usize {
            out.fill(1);
            out.len() / 2
        }
    }

    /// The resampled queue is a hand-off buffer, never a backlog: it
    /// holds what the fill asked for and no more, however much source
    /// is waiting. Converting everything available instead put the
    /// session's whole audio backlog here — downstream of the servo,
    /// which measures the *source* queue — so it grew unchecked (until
    /// a cap started shedding whole spans, audible as skipping) while
    /// the servo sat pinned at full authority chasing a level it could
    /// not move.
    #[test]
    fn the_resampled_queue_holds_only_what_was_asked_for() {
        let mut pull = Resampled::new(Endless);
        for _ in 0..200 {
            pull.source_available();
            // 4x fast-forward: 48 kHz device, 192 kHz claimed.
            pull.process(32768.0, 192_000.0, 512);
            let mut out = [0i16; 1024];
            pull.read(&mut out, 512);
        }
        assert!(
            pull.available() <= 512,
            "resampled queue grew to {} frames",
            pull.available()
        );
    }

    /// A console that accounts for its own buffer, the way mgba's core
    /// can. The level is shared so a test can see what stayed behind.
    struct Reporting(std::sync::Arc<std::sync::Mutex<usize>>);

    impl AudioDrain for Reporting {
        fn sample_rate(&self) -> f64 {
            32768.0
        }

        fn queued(&mut self) -> usize {
            *self.0.lock().unwrap()
        }

        fn drain(&mut self, out: &mut [i16]) -> usize {
            let mut level = self.0.lock().unwrap();
            let frames = (*level).min(out.len() / 2);
            *level -= frames;
            out[..frames * 2].fill(1);
            frames
        }
    }

    /// A console that can account for its own backlog keeps it: that
    /// buffer is what a rollback revokes speculated audio out of, and
    /// every frame pulled out early is a frame the mispredict can no
    /// longer take back — it plays anyway, and its corrected
    /// re-simulation gets swallowed to avoid an echo. So a fill takes
    /// only what it is about to play.
    #[test]
    fn a_console_that_reports_its_level_keeps_its_backlog() {
        let level = std::sync::Arc::new(std::sync::Mutex::new(20_000));
        let mut pull = Resampled::new(Reporting(level.clone()));

        assert_eq!(pull.source_available(), 20_000);
        pull.process(32768.0, 48_000.0, 512);
        let mut out = [0i16; 1024];
        assert_eq!(pull.read(&mut out, 512), 512);

        // 512 device frames at 48 kHz is ~350 frames of a 32768 Hz
        // console, so that is about all that should have moved.
        let left = *level.lock().unwrap();
        assert!(left > 19_500, "console was drained down to {left} frames");
        assert_eq!(pull.source_available(), 20_000 - 512 * 32768 / 48_000);
    }

    /// A console with a fixed batch of audio and nothing more, at BN4+'s
    /// doubled rate.
    struct Burst(usize);

    impl AudioDrain for Burst {
        fn sample_rate(&self) -> f64 {
            65536.0
        }

        fn queued(&mut self) -> usize {
            self.0
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
        // The speed folds into the destination rate:
        // 48000 × (59.7275 / 240).
        pull.process(65536.0, 11945.5, 512);
        assert_eq!(pull.available(), 2);
    }

    /// A zero or negative step can never advance the cursor, so the
    /// resample loop would push output forever.
    #[test]
    fn a_degenerate_rate_produces_nothing_rather_than_hanging() {
        let mut pull = Resampled::new(Endless);
        pull.source_available();
        pull.process(0.0, 48_000.0, 512);
        assert_eq!(pull.available(), 0);
        pull.process(32768.0, 0.0, 512);
        assert_eq!(pull.available(), 0);
    }
}
