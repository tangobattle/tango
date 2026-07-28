//! Pulling audio out of a DS pair.
//!
//! The DS SPU mixes at a fixed rate ([`SAMPLE_RATE`](crate::SAMPLE_RATE))
//! and hands out interleaved stereo directly, so there is no per-core
//! buffer plumbing to do — draining is the whole implementation, and a
//! host resamples from the rate the seam reports.

use tango_match::Backend;

/// Drain one console's audio as interleaved stereo, returning frames
/// written.
pub fn drain(link: &mut crate::Link, player: usize, out: &mut [i16]) -> usize {
    crate::MelonDs::drain_audio(link, player, out)
}

/// The rate those samples are at.
pub fn sample_rate() -> f64 {
    crate::SAMPLE_RATE
}
