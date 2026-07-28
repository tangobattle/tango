//! Getting a console's sound to a device.
//!
//! A console produces at its own rate and a sound device wants another,
//! so something has to resample — and a host also nudges the *claimed*
//! source rate to servo its output queue toward a target level, which
//! is how a session keeps latency steady without pitch-shifting.
//!
//! That servo is engine-neutral arithmetic. What is not is the
//! resampling itself: mgba ships one tuned to its cores' buffers, while
//! a DS hands over interleaved stereo at a fixed rate. So the contract
//! lives here and each backend answers it.

/// Cross-thread pull of one console's audio, for a host's sound
/// callback.
///
/// A host mid-boot may have no console yet; an implementation is free
/// to report nothing available and produce silence.
pub trait AudioPull: Send {
    /// The rate this console truly produces at, in Hz.
    fn sample_rate(&self) -> f64;

    /// Frames the console has produced and not yet had consumed. The
    /// host's servo reads this to decide how hard to trim.
    fn source_available(&self) -> usize;

    /// Consume from the console as if it produced at
    /// `claimed_source_rate`, producing at `destination_rate`.
    ///
    /// Claiming a rate above the true one makes each output frame eat
    /// more input (draining the queue); below, less (refilling it).
    fn process(&mut self, claimed_source_rate: f64, destination_rate: f64);

    /// Frames ready for the device.
    fn available(&self) -> usize;

    /// Take up to `frames` frames as interleaved stereo, returning how
    /// many were written.
    fn read(&mut self, out: &mut [i16], frames: usize) -> usize;
}
