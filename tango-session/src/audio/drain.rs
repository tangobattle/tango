//! Getting a console's sound out of it.
//!
//! What a session plays is whatever a running console has produced, and
//! reaching a console means reaching past the lock its simulation ticks
//! under. [`Drain`] is what everything above works in terms of — a
//! thing with a rate that hands over samples — and [`side_drain`] is
//! the one implementation over a real console, built on the
//! [`SideSource`] every engine's boot hands out.
//!
//! One implementation, not one per engine: every console needs exactly
//! the same discipline applied to it, and that discipline is the whole
//! content of this file.

use std::sync::atomic::{AtomicU64, Ordering};

use tango_match::SideSource;

/// Raw audio out of one console.
pub trait Drain: Send {
    /// The rate this console produces at, in Hz.
    fn sample_rate(&self) -> f64;

    /// Take up to `out`'s worth of what the console has produced, as
    /// interleaved stereo, and answer with how much it had in total —
    /// frames, counting both what went into `out` and what would not
    /// fit. Taking it consumes it, which is what stops a session
    /// replaying audio it already played after a rollback.
    ///
    /// The total rather than what landed, because the split is the
    /// caller's own arithmetic — a drain fills `out` as far as it goes,
    /// so `min(total, out.len() / 2)` frames landed — while the rest is
    /// audio that exists and is coming, which a caller counting its
    /// backlog has to count and could not otherwise see.
    ///
    /// `None` for a console that could not be reached at all: its lock
    /// was held by the thread ticking it (see [`side_drain`]). Nothing
    /// was written and nothing is known — which is a different answer
    /// from `Some(0)`, a console reached and found empty. A caller
    /// steering on the level keeps the last one it saw rather than
    /// reading a stall that isn't there.
    fn drain(&mut self, out: &mut [i16]) -> Option<usize>;
}

/// The one drain over a real console, whichever seat of whichever boot
/// the [`SideSource`] reaches.
pub fn side_drain(source: Box<dyn SideSource>) -> Box<dyn Drain> {
    // Seed the rate fallback while the console is reachable:
    // construction runs off the sound callback, so a blocking wait is
    // fine here and never again after.
    let mut rate = 0.0;
    source.with_side(&mut |side| rate = side.audio_sample_rate());
    Box::new(ConsoleAudio {
        source,
        last_rate: AtomicU64::new(rate.to_bits()),
    })
}

/// A console behind whichever lock its simulation ticks under. Every
/// read goes through the source's `try_side`, never a blocking wait:
/// this runs on the sound callback, and the lock it wants is the one
/// the simulation holds — where a tick is a console (or two) emulating
/// a whole frame, and a rollback puts a multi-megabyte restore in
/// front of that. Waiting is how a callback misses its deadline, and a
/// callback that misses its deadline is a click. The console holds the
/// backlog, so skipping a take costs nothing but the audio arriving
/// one fill later.
struct ConsoleAudio {
    source: Box<dyn SideSource>,
    /// Last successfully read sample rate, as f64 bits — served
    /// whenever a read lands while the drive thread holds the console
    /// mid-tick. The stream re-reads the rate every fill (BN4+ carts
    /// flip theirs after boot) and the whole resample ratio is built on
    /// it, so a placeholder would rebase playback for that fill; a
    /// stale reading is only ever one fill old at a real change.
    last_rate: AtomicU64,
}

impl Drain for ConsoleAudio {
    fn sample_rate(&self) -> f64 {
        let mut rate = 0.0;
        // Zero also covers a core that reports no rate yet: either way
        // the resampler must not divide by it.
        if self.source.try_side(&mut |side| rate = side.audio_sample_rate()) && rate > 0.0 {
            self.last_rate.store(rate.to_bits(), Ordering::Relaxed);
            rate
        } else {
            f64::from_bits(self.last_rate.load(Ordering::Relaxed))
        }
    }

    fn drain(&mut self, out: &mut [i16]) -> Option<usize> {
        let mut total = 0;
        self.source
            .try_side(&mut |side| total = side.drain_audio(out))
            .then_some(total)
    }
}
