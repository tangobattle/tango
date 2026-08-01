//! A console's audio on its way to a device.
//!
//! What a console hands over ([`Drain`]) is raw: its own rate, whatever
//! it has produced so far, and how much of that would not fit in one
//! take. Turning that into device-rate frames at a chosen speed is the
//! host's job, and every console needs the same job done, so it is done
//! once here rather than per emulator.
//!
//! One decision lives in this file and nowhere else: how much to
//! resample. Exactly what the caller is about to play, and no more — the
//! backlog then sits on the unresampled side, which is the side
//! [`Stream`](super::Stream) measures and steers. Converting
//! everything queued instead would put it downstream of the only thing
//! regulating it, where it grows until something has to shed it.
//!
//! The backlog is held here rather than left in the console. Consoles
//! keep small rings — melonDS's SPU holds about 43 ms, less than a
//! session queues — so what a host can leave behind is neither
//! knowable in advance nor reliably enough, and a level that saturates
//! below the target the servo is steering toward is a servo that never
//! arrives.

use super::Drain;

/// Ceiling on one drain, in frames — a bound on the scratch buffer and
/// on how much a single call can move. Sized so one successful reach
/// can carry several *fast-forward* fills' worth of a 65536 Hz cart
/// (~2800 source frames per fill at 4x), not merely one: reaches fail
/// whenever the drive thread holds the console's lock, and at 4x that
/// is a large fraction of wall time — the successful reaches between
/// misses have to catch the intake up, or the staged queue starves at
/// full speed no matter how deep the servo holds it.
const DRAIN_CHUNK: usize = 16384;

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
    drain: Box<dyn Drain>,
    /// Taken from the console, not yet resampled. This is a session's
    /// audio backlog: it is what [`source_available`](Self::source_available)
    /// reports, so it is the queue a host's rate control steers.
    source: Vec<i16>,
    /// Fractional position into `source`, in frames.
    cursor: f64,
    /// Resampled, waiting for the device — a hand-off buffer, never
    /// more than the last [`process`](Resampler::process) was asked for.
    dest: Vec<i16>,
    /// Reusable landing buffer for [`Drain::drain`]. A sound callback
    /// must not allocate.
    scratch: Vec<i16>,
    /// What the last reach left behind in the console, and what an
    /// unreachable one is answered with. A reach that finds the
    /// console's lock held takes nothing and learns nothing, but the
    /// backlog is still sitting there — reporting zero for it would
    /// read as a stalled sim to the servo above, which answers a stall
    /// with a whole target's worth of silence.
    left: usize,
}

impl Resampler {
    pub fn new(drain: Box<dyn Drain>) -> Self {
        Resampler {
            drain,
            source: Vec::new(),
            cursor: 0.0,
            dest: Vec::new(),
            scratch: Vec::new(),
            left: 0,
        }
    }

    /// Frames still to be resampled out of `source`.
    fn staged(&self) -> usize {
        (self.source.len() / 2).saturating_sub(self.cursor as usize)
    }

    /// Take what the console has, and report anything it could not hand
    /// over in one go. One reach into the console per fill, because that
    /// is the expensive part: a sound callback shares a lock with the
    /// simulation, so every reach is a chance to stall behind a tick.
    fn take(&mut self) -> usize {
        self.scratch.resize(DRAIN_CHUNK * 2, 0);
        if let Some(total) = self.drain.drain(&mut self.scratch) {
            // The drain fills the scratch as far as it goes, so what
            // landed is the total or a chunk of it, and the rest is
            // still sitting in the console.
            let written = total.min(DRAIN_CHUNK);
            self.source.extend_from_slice(&self.scratch[..written * 2]);
            self.left = total - written;
        }
        self.left
    }

    /// The rate the console truly produces at, in Hz.
    pub fn sample_rate(&self) -> f64 {
        self.drain.sample_rate()
    }

    pub fn source_available(&mut self) -> usize {
        // Counting what stayed behind as well: a drain capped by its own
        // buffer has still produced that audio, and a level that ignored
        // it would read low and be steered against.
        let left = self.take();
        self.staged() + left
    }

    pub fn process(&mut self, claimed_source_rate: f64, destination_rate: f64, frames: usize) {
        let step = claimed_source_rate / destination_rate;
        // A non-positive or non-finite step would never advance the
        // cursor, so the loop below would push forever.
        if !step.is_finite() || step <= 0.0 {
            return;
        }
        // Resamples out of what is already staged: the console was
        // reached once, in `source_available`, and reaching it again
        // here is another chance to stall behind a tick.
        let want = frames.saturating_sub(self.dest.len() / 2);
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

    pub fn available(&self) -> usize {
        self.dest.len() / 2
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
            let chunk = rest.min(DRAIN_CHUNK);
            self.scratch.resize(chunk * 2, 0);
            let Some(total) = self.drain.drain(&mut self.scratch) else {
                break;
            };
            let got = total.min(chunk);
            self.left = total - got;
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

    /// A console that always has audio ready, at a GBA-ish rate.
    struct Endless;

    impl Drain for Endless {
        fn sample_rate(&self) -> f64 {
            32768.0
        }

        fn drain(&mut self, out: &mut [i16]) -> Option<usize> {
            out.fill(1);
            Some(out.len() / 2 + DRAIN_CHUNK)
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

    impl Drain for Reporting {
        fn sample_rate(&self) -> f64 {
            32768.0
        }

        fn drain(&mut self, out: &mut [i16]) -> Option<usize> {
            let mut level = self.0.lock().unwrap();
            let total = *level;
            let written = total.min(out.len() / 2);
            *level -= written;
            out[..written * 2].fill(1);
            Some(total)
        }
    }

    /// The backlog comes out of the console and is held here, where it
    /// is exactly measurable and always reachable. What the console can
    /// spare is not a host's to guess at, and a level that saturates
    /// below the target a servo steers toward is a servo that never
    /// arrives.
    #[test]
    fn the_backlog_comes_out_of_the_console() {
        let level = std::sync::Arc::new(std::sync::Mutex::new(20_000));
        let mut pull = Resampler::new(Box::new(Reporting(level.clone())));

        // Reported whole, wherever it physically sits.
        assert_eq!(pull.source_available(), 20_000);
        pull.process(32768.0, 48_000.0, 512);
        let mut out = [0i16; 1024];
        assert_eq!(pull.read(&mut out, 512), 512);

        // Nothing lost in the move: backlog is what was there minus what
        // played.
        assert_eq!(pull.source_available(), 20_000 - 512 * 32768 / 48_000);
    }

    /// A console reachable only on some calls — a stand-in for one
    /// whose buffer sits behind the lock its simulation ticks under,
    /// which is every console here.
    struct Intermittent {
        level: usize,
        call: usize,
    }

    impl Drain for Intermittent {
        fn sample_rate(&self) -> f64 {
            32768.0
        }

        fn drain(&mut self, out: &mut [i16]) -> Option<usize> {
            self.call += 1;
            // Produce a fill's worth per call, reachable on one call in
            // three.
            self.level += 350;
            if self.call % 3 != 0 {
                return None;
            }
            let total = self.level;
            let written = total.min(out.len() / 2);
            self.level -= written;
            out[..written * 2].fill(1);
            Some(total)
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

    impl Drain for Cramped {
        fn sample_rate(&self) -> f64 {
            48_000.0
        }

        fn drain(&mut self, out: &mut [i16]) -> Option<usize> {
            self.level += 800;
            // What a small ring does when it overflows: keeps the newest
            // and drops the oldest, silently.
            self.level = self.level.min(self.capacity);
            let total = self.level;
            let written = total.min(out.len() / 2);
            self.level -= written;
            out[..written * 2].fill(1);
            Some(total)
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
    }

    /// A console with a fixed batch of audio and nothing more, at BN4+'s
    /// doubled rate.
    struct Burst(usize);

    impl Drain for Burst {
        fn sample_rate(&self) -> f64 {
            65536.0
        }

        fn drain(&mut self, out: &mut [i16]) -> Option<usize> {
            let total = self.0;
            let written = total.min(out.len() / 2);
            self.0 -= written;
            out[..written * 2].fill(1);
            Some(total)
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
