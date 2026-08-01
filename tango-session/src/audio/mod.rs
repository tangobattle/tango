//! Audio: what a host's output stream pulls from.
//!
//! [`Source`] is the seam — a host's output device pulls frames from
//! one — and [`Stream`] is the only implementation a session needs: it
//! takes the consuming end of the ring the simulation pushes into
//! ([`AudioOut`](tango_match::AudioOut)), owns the resampler over it,
//! and runs the dynamic rate control that decides how fast to play (see
//! [`stream`] for why that's the whole design).
//!
//! All of it lives above the engine seam, because none of it is an
//! emulator's business: a backend answers for one console's rate and
//! its sample queue ([`Side`](tango_match::Side)) and nothing else, and
//! the ring is what carries that across to the thread a device callback
//! runs on — without the callback ever reaching for a console, so a
//! loaded drive loop cannot starve the sound by holding them.

mod resampler;
pub mod stream;

pub use resampler::Resampler;
pub use stream::Stream;

pub const NUM_CHANNELS: usize = tango_match::audio::CHANNELS;
pub const SAMPLES: usize = 512;

/// How much audio the ring between the simulation and the device can
/// hold, in frames.
///
/// Far more than anything ever plays out of it: the stream holds ~50 ms
/// and sheds whatever gets past three times that, so at a GBA's
/// 32768 Hz this is two seconds of headroom over a queue that never
/// legitimately reaches a twentieth of it. The headroom is where a burst
/// lands — a replay seek chase, a device stall's catch-up — so the
/// stream gets to shed it deliberately from the oldest end rather than
/// the ring dropping the newest on the producer's behalf. 256 KB per
/// session, which is nothing beside a console's savestates.
pub const RING_FRAMES: usize = 65536;

/// A session's audio ring, as the two ends that share it: the producing
/// end for whatever boots the pair, the consuming end for the [`Stream`]
/// the host plays.
///
/// Made before the pair exists, which is what lets a host bind its
/// output stream at session construction while the boot — a priming
/// walk, seconds on a DS-class game — is still running. An unfed ring
/// reads empty, so the stream simply primes until the pair comes up.
pub fn ring() -> (tango_match::AudioIn, tango_match::AudioOut) {
    tango_match::audio::channel(RING_FRAMES)
}

/// Whatever a host's output device pulls its frames from. A session
/// hands over a [`Stream`]; a host may put its own between (the
/// desktop's late-binding mux, so the device outlives any one session).
pub trait Source {
    fn fill(&mut self, buf: &mut [[i16; NUM_CHANNELS]]) -> usize;
}
