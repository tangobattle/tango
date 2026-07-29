//! A console's audio on its way to a device.
//!
//! What a backend hands over ([`tango_match::AudioDrain`]) is raw: this
//! console's own rate, whatever it has produced so far, and how much
//! room its buffer has. Turning that into device-rate frames at a
//! chosen speed is the host's job, and every console needs the same job
//! done, so it is done once here rather than per emulator.
//!
//! Two decisions live in this file and nowhere else. How much of the
//! backlog to take out of the console — as little as possible, because
//! that buffer is what a rollback revokes speculated audio out of, but
//! never so little that a fill goes hungry. And how much to resample —
//! exactly what the caller is about to play, so the backlog stays on the
//! unresampled side where [`CoreStream`](super::CoreStream) can see and
//! steer it.

use tango_match::AudioDrain;

/// How much console audio to keep staged locally, in seconds.
///
/// The rest of the backlog stays in the console's buffer, where a
/// rollback's revocation can still reach it — but not all of it can.
/// Staging nothing would leave every fill depending on the console
/// having samples ready at that instant, and a fill that comes up short
/// is a gap in the sound. This is the insurance against that: it covers
/// three or so fills, so a console that cannot be reached for a couple
/// of them in a row costs nothing. It is still the minority of the
/// backlog a host holds, so most of that stays revocable.
const STAGED_RESERVE_SECS: f64 = 0.04;

/// Ceiling on one drain, in frames — a bound on the scratch buffer and
/// on how much a single call can move.
const DRAIN_CHUNK: usize = 4096;

/// A console's audio on its way to a device: staged, resampled, and
/// accounted for.
///
/// Not a trait and not a wrapper around one — a host owns one of these
/// and drives it. Every console needs the same thing done to its audio,
/// so this is the one implementation rather than something each backend
/// re-answers.
///
/// Linear interpolation over a fractional cursor: deliberately the
/// simplest thing that honours a claimed rate, since the cursor
/// advances by `claimed / destination` per output frame and the queue
/// therefore drifts exactly the way a servo intends.
pub struct Resampler {
    drain: Box<dyn AudioDrain>,
    /// Taken from the console, not yet resampled — a working reserve of
    /// [`STAGED_RESERVE_SECS`], no more. The rest of the backlog stays in
    /// the console's own buffer, where a rollback's revocation can still
    /// reach it.
    source: Vec<i16>,
    /// Fractional position into `source`, in frames.
    cursor: f64,
    /// Resampled, waiting for the device — a hand-off buffer, never
    /// more than the last [`process`](Resampler::process) was asked for.
    dest: Vec<i16>,
    /// Reusable landing buffer for [`AudioDrain::drain`]. A sound
    /// callback must not allocate.
    scratch: Vec<i16>,
    /// Source frames the last [`process`](Resampler::process) got
    /// through — what the local reserve is sized against, so it follows
    /// a speed change instead of being set for 1x and starving.
    appetite: usize,
    /// The console's own buffer as of the last drain: how full, and how
    /// big. Together they say how much backlog may safely be left there.
    console_queued: usize,
    console_capacity: usize,
}

impl Resampler {
    pub fn new(drain: Box<dyn AudioDrain>) -> Self {
        Resampler {
            drain,
            source: Vec::new(),
            cursor: 0.0,
            dest: Vec::new(),
            scratch: Vec::new(),
            appetite: 0,
            console_queued: 0,
            console_capacity: 0,
        }
    }

    /// Frames still to be resampled out of `source`.
    fn staged(&self) -> usize {
        (self.source.len() / 2).saturating_sub(self.cursor as usize)
    }

    /// Top `source` up to the local reserve out of the console, and
    /// report what the console still holds. One reach into the console,
    /// because that is the expensive part.
    fn top_up(&mut self) -> usize {
        // Time alone doesn't size this: a fast-forwarded fill eats
        // several times the source a 1x one does, so the reserve also
        // tracks what the last fill actually got through.
        let reserve = ((self.drain.sample_rate() * STAGED_RESERVE_SECS) as usize).max(self.appetite * 2);
        // Whatever the console cannot safely keep comes here too. A ring
        // allowed to fill overwrites or drops, so half of one is as much
        // backlog as may be left in it — and some consoles hold barely
        // more than that half to begin with, in which case essentially
        // all of the backlog has to live on this side.
        let overflowing = self.console_queued.saturating_sub(self.console_capacity / 2);
        let short = reserve
            .saturating_sub(self.staged())
            .max(overflowing)
            .min(DRAIN_CHUNK);
        // Still asks even when the reserve is full: the level has to
        // come back either way, and a zero-length take is the cheap way
        // to get it in the same call.
        self.scratch.resize(short * 2, 0);
        let drained = self.drain.drain(&mut self.scratch);
        self.source.extend_from_slice(&self.scratch[..drained.written * 2]);
        self.console_queued = drained.queued;
        self.console_capacity = drained.capacity;
        drained.queued
    }

    /// The rate the console truly produces at, in Hz.
    pub fn sample_rate(&self) -> f64 {
        self.drain.sample_rate()
    }

    pub fn source_available(&mut self) -> usize {
        // The backlog is mostly counted rather than taken — it stays in
        // the console's buffer, within reach of a rollback's revocation
        // — but a few tens of ms are staged here so a fill is never at
        // the mercy of what the console happens to have this instant.
        let queued = self.top_up();
        self.staged() + queued
    }

    pub fn process(&mut self, claimed_source_rate: f64, destination_rate: f64, frames: usize) {
        let step = claimed_source_rate / destination_rate;
        // A non-positive or non-finite step would never advance the
        // cursor, so the loop below would push forever.
        if !step.is_finite() || step <= 0.0 {
            return;
        }
        // Resamples out of what is already staged — the console was
        // reached once, in `source_available`, and reaching it again
        // here is another chance to stall behind a tick. Worth it only
        // when the alternative is a short fill, which is a gap in the
        // sound: the reserve normally covers this, and doesn't only when
        // the appetite it is sized against has just jumped.
        let want = frames.saturating_sub(self.dest.len() / 2);
        let need = (self.cursor + want as f64 * step).ceil().max(0.0) as usize + 2;
        if need > self.source.len() / 2 {
            let short = (need - self.source.len() / 2).min(DRAIN_CHUNK);
            self.scratch.resize(short * 2, 0);
            let drained = self.drain.drain(&mut self.scratch);
            self.source.extend_from_slice(&self.scratch[..drained.written * 2]);
            self.console_queued = drained.queued;
            self.console_capacity = drained.capacity;
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
        self.appetite = consumed;
        if consumed > 0 {
            self.source.drain(..consumed * 2);
            self.cursor -= consumed as f64;
        }
    }

    pub fn available(&self) -> usize {
        self.dest.len() / 2
    }

    pub fn framerate_ratio(&self, fps_target: f64) -> f64 {
        self.drain.framerate_ratio(fps_target)
    }

    pub fn discard_source(&mut self, frames: usize) {
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
            let got = self.drain.drain(&mut self.scratch).written;
            if got == 0 {
                break;
            }
            rest -= got;
        }
    }

    pub fn read(&mut self, out: &mut [i16], frames: usize) -> usize {
        let frames = frames.min(self.dest.len() / 2).min(out.len() / 2);
        out[..frames * 2].copy_from_slice(&self.dest[..frames * 2]);
        self.dest.drain(..frames * 2);
        frames
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tango_match::Drained;

    /// A console that always has audio ready, at a GBA-ish rate.
    struct Endless;

    impl AudioDrain for Endless {
        fn sample_rate(&self) -> f64 {
            32768.0
        }

        fn drain(&mut self, out: &mut [i16]) -> Drained {
            out.fill(1);
            Drained {
                written: out.len() / 2,
                queued: DRAIN_CHUNK,
                capacity: DRAIN_CHUNK * 8,
            }
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
        let mut pull = Resampler::new(Box::new(Endless));
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

        fn drain(&mut self, out: &mut [i16]) -> Drained {
            let mut level = self.0.lock().unwrap();
            let written = (*level).min(out.len() / 2);
            *level -= written;
            out[..written * 2].fill(1);
            Drained {
                written,
                queued: *level,
                capacity: 1 << 20,
            }
        }
    }

    /// The backlog stays in the console, bar a small local reserve. That
    /// buffer is what a rollback revokes speculated audio out of, and
    /// every frame taken early is a frame the mispredict can no longer
    /// take back — it plays anyway, and its corrected re-simulation gets
    /// swallowed to avoid an echo. The reserve is the deliberate
    /// exception: without something staged, a fill would depend on the
    /// console being ready at that instant, and a short fill is a gap.
    #[test]
    fn a_console_keeps_its_backlog_bar_the_reserve() {
        let level = std::sync::Arc::new(std::sync::Mutex::new(20_000));
        let mut pull = Resampler::new(Box::new(Reporting(level.clone())));

        // Reported whole, wherever it physically sits.
        assert_eq!(pull.source_available(), 20_000);
        pull.process(32768.0, 48_000.0, 512);
        let mut out = [0i16; 1024];
        assert_eq!(pull.read(&mut out, 512), 512);

        // Only the reserve came out, and the fill ate ~350 frames of it
        // (512 device frames at 48 kHz against a 32768 Hz console).
        let reserve = (32768.0 * STAGED_RESERVE_SECS) as usize;
        assert_eq!(*level.lock().unwrap(), 20_000 - reserve);
        assert!(pull.staged() < reserve, "staging never gave anything up");
        // Nothing lost in the split: backlog is what was there minus
        // what played.
        assert_eq!(pull.source_available(), 20_000 - 512 * 32768 / 48_000);
    }

    /// A console that only hands audio over on some calls — a stand-in
    /// for one whose buffer sits behind the lock its simulation ticks
    /// under, which is every console here.
    struct Intermittent {
        level: usize,
        call: usize,
    }

    impl AudioDrain for Intermittent {
        fn sample_rate(&self) -> f64 {
            32768.0
        }

        fn drain(&mut self, out: &mut [i16]) -> Drained {
            self.call += 1;
            // Produce a fill's worth per call, hand it over on one call
            // in three.
            self.level += 350;
            if self.call % 3 != 0 {
                return Drained {
                    written: 0,
                    queued: self.level,
                    capacity: 1 << 20,
                };
            }
            let written = self.level.min(out.len() / 2);
            self.level -= written;
            out[..written * 2].fill(1);
            Drained {
                written,
                queued: self.level,
                capacity: 1 << 20,
            }
        }
    }

    /// Reaching the console is not something a fill can count on, so a
    /// local reserve stands in for it. Without one, every unreachable
    /// call is a fill that comes up short — and a short fill is a gap in
    /// the sound, and a host that treats it as a stall will answer with
    /// far more silence than that.
    #[test]
    fn an_unreachable_console_does_not_short_a_fill() {
        let mut pull = Resampler::new(Box::new(Intermittent { level: 4000, call: 0 }));
        let mut out = [0i16; 1024];
        for i in 0..500 {
            pull.source_available();
            pull.process(32768.0, 48_000.0, 512);
            let got = pull.read(&mut out, 512);
            // The first reach may land on an unreachable call, which is
            // what a host's priming covers; after that, never.
            if i > 4 {
                assert_eq!(got, 512, "fill {i} came up short");
            }
        }
    }

    /// A console whose buffer is far smaller than the backlog a host
    /// wants to hold — melonDS's SPU ring is about 43 ms, against the
    /// ~120 ms a session queues.
    struct Cramped {
        level: usize,
        capacity: usize,
    }

    impl AudioDrain for Cramped {
        fn sample_rate(&self) -> f64 {
            48_000.0
        }

        fn drain(&mut self, out: &mut [i16]) -> Drained {
            self.level += 800;
            // What a small ring does when it overflows: keeps the newest
            // and drops the oldest, silently.
            self.level = self.level.min(self.capacity);
            let written = self.level.min(out.len() / 2);
            self.level -= written;
            out[..written * 2].fill(1);
            Drained {
                written,
                queued: self.level,
                capacity: self.capacity,
            }
        }
    }

    /// Backlog can only be left in a console that can hold it. Leaving
    /// it in one that cannot is two failures at once: the ring overflows
    /// and destroys audio, and the level a host reads back saturates
    /// below the target it is trying to reach — so its queue never fills,
    /// its priming never releases, and the session plays silence forever.
    /// So whatever will not fit comes out and is staged here.
    #[test]
    fn a_cramped_console_hands_its_backlog_over() {
        let mut pull = Resampler::new(Box::new(Cramped {
            level: 0,
            capacity: 2048,
        }));
        let mut out = [0i16; 1024];
        // A host builds its queue by holding silence while the console
        // produces — so what it can accumulate is the question.
        for _ in 0..40 {
            pull.source_available();
        }
        // Then plays, and must not give it back.
        for _ in 0..200 {
            pull.source_available();
            pull.process(48_000.0, 48_000.0, 512);
            pull.read(&mut out, 512);
        }
        // A 120 ms backlog at 48 kHz is 5760 frames — nearly three times
        // what this console can hold, so it has to be reachable through
        // the staged side.
        assert!(
            pull.source_available() > 5760,
            "backlog saturated at {} frames, below a target it must be able to reach",
            pull.source_available()
        );
        assert!(
            pull.console_queued <= 2048,
            "console was left overflowing at {} frames",
            pull.console_queued
        );
    }

    /// A console with a fixed batch of audio and nothing more, at BN4+'s
    /// doubled rate.
    struct Burst(usize);

    impl AudioDrain for Burst {
        fn sample_rate(&self) -> f64 {
            65536.0
        }

        fn drain(&mut self, out: &mut [i16]) -> Drained {
            let written = self.0.min(out.len() / 2);
            self.0 -= written;
            out[..written * 2].fill(1);
            Drained {
                written,
                queued: self.0,
                capacity: 1 << 20,
            }
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
        let mut pull = Resampler::new(Box::new(Burst(8)));
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
        let mut pull = Resampler::new(Box::new(Endless));
        pull.source_available();
        pull.process(0.0, 48_000.0, 512);
        assert_eq!(pull.available(), 0);
        pull.process(32768.0, 0.0, 512);
        assert_eq!(pull.available(), 0);
    }
}
