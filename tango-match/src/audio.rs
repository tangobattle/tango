//! Where a console's sound comes out.
//!
//! Only the seam lives here, and only because this crate's own
//! [`Backend`](crate::Backend) and `Running*` traits are what hand it
//! out — a backend has to be able to answer "here is my audio" in a
//! vocabulary this crate can name. What a host then *does* with it —
//! staging, resampling, rate control — is a host's business and lives
//! with the host.
//!
//! So this is deliberately thin: raw interleaved stereo out of one
//! console, plus the facts about the buffer it came from that a caller
//! cannot otherwise know.

/// What one [`AudioDrain::drain`] handed over, and what stayed behind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Drained {
    /// Frames written into the caller's buffer.
    pub written: usize,
    /// Frames still in the console's own buffer afterwards — what one
    /// call could not hand over. Audio that exists and is coming, so a
    /// caller counting its backlog has to count this too.
    pub queued: usize,
}

/// Raw audio out of one console.
pub trait AudioDrain: Send {
    /// The rate this console produces at, in Hz.
    fn sample_rate(&self) -> f64;

    /// Take up to `out`'s worth of what the console has produced, as
    /// interleaved stereo, and report what is left behind. Taking it
    /// consumes it — which is what stops a session replaying audio it
    /// already played after a rollback.
    ///
    /// Both facts come back together because a caller wants them
    /// together and reaching a console can be expensive: an emulator's
    /// audio typically sits behind the same lock its simulation ticks
    /// under, so a sound callback asking twice is a second chance to
    /// stall behind a whole frame of emulation — and a sound callback
    /// that misses its deadline is a click.
    ///
    /// What is left behind comes back because a caller cannot otherwise
    /// tell "the console has nothing" from "the console had more than
    /// this call could carry", and those want opposite responses.
    fn drain(&mut self, out: &mut [i16]) -> Drained;

    /// How production scales when the host paces the simulation at
    /// `fps_target` — a throttled sim stretches playback by the same
    /// ratio instead of starving it, and a fast-forwarded one
    /// compresses. 1.0 means the console runs at its own rate.
    fn framerate_ratio(&self, _fps_target: f64) -> f64 {
        1.0
    }
}
