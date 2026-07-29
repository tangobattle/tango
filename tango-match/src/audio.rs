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
    /// Frames still in the console's own buffer afterwards.
    pub queued: usize,
    /// Frames that buffer holds in total.
    ///
    /// How much backlog may be left in it, which is not a matter of
    /// taste: these are small rings that overwrite or drop when full, so
    /// leaving more than one holds silently destroys audio. They differ
    /// by an order of magnitude between consoles — a GBA core's buffer
    /// is whatever the host asked for, while melonDS's SPU sizes itself
    /// to about a 40 ms of output and no more — so a caller that wants
    /// to leave a backlog behind has to ask rather than assume.
    pub capacity: usize,
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
    /// What is left behind matters because that is where a session's
    /// backlog should sit: a rollback revokes speculated audio out of
    /// the console's own buffer, so anything drained early is beyond its
    /// reach — the mispredicted span plays anyway, and its corrected
    /// re-simulation has to be swallowed to avoid an echo.
    fn drain(&mut self, out: &mut [i16]) -> Drained;

    /// How production scales when the host paces the simulation at
    /// `fps_target` — a throttled sim stretches playback by the same
    /// ratio instead of starving it, and a fast-forwarded one
    /// compresses. 1.0 means the console runs at its own rate.
    fn framerate_ratio(&self, _fps_target: f64) -> f64 {
        1.0
    }
}
