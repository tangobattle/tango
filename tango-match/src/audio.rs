//! Where a console's sound comes out.
//!
//! The seam lives here — a link has to be able to answer "here is my
//! audio" in a vocabulary this crate can name — plus the one drain
//! implementation every engine shares, built on the [`Link`] audio
//! primitives. What a host then *does* with it — staging, resampling,
//! rate control — is a host's business and lives with the host.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::link::Link;

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

/// One console of a shared link, as the raw source a host resamples —
/// the drain every engine's match hands out.
pub fn console_audio(link: Arc<Mutex<dyn Link>>, player: Arc<AtomicUsize>) -> Box<dyn AudioDrain> {
    // Seed the rate fallback while the link is reachable: construction
    // runs off the sound callback, so a blocking lock is fine here and
    // never again after. The ratio starts at the native 1.0 — there is
    // no target rate to ask about yet.
    let rate = {
        let mut link = link.lock().unwrap();
        let player = player.load(Ordering::Relaxed);
        link.audio_sample_rate(player)
    };
    Box::new(ConsoleAudio {
        link,
        player,
        last_rate: AtomicU64::new(rate.to_bits()),
        last_ratio: AtomicU64::new(1.0f64.to_bits()),
        last_queued: AtomicUsize::new(0),
    })
}

/// A link behind the engine's shared handle. Every read goes through a
/// `try_lock`, never a blocking one: this runs on the sound callback,
/// and the mutex it wants is the one the simulation ticks under —
/// where a tick is two consoles each emulating a whole frame, and a
/// rollback puts a multi-megabyte restore in front of that. Waiting is
/// how a callback misses its deadline, and a callback that misses its
/// deadline is a click. The console holds the backlog, so skipping a
/// take costs nothing but the audio arriving one fill later.
struct ConsoleAudio {
    link: Arc<Mutex<dyn Link>>,
    /// Read per fill, so a host that moves the sound between seats
    /// (training's side swap) does so without the resampler above
    /// being rebuilt under it.
    player: Arc<AtomicUsize>,
    /// Last successfully read sample rate and framerate ratio, as f64
    /// bits — served whenever a read lands while the drive thread
    /// holds the pair mid-tick. The stream re-reads both every fill,
    /// and it watches the ratio for the jump that means a deliberate
    /// speed change; a fast-forwarded drive loop holds the pair for a
    /// large fraction of wall time, so a placeholder on those fills
    /// would read as the user toggling speed off and on again,
    /// snapping playback rate back and forth. A stale reading is only
    /// ever one fill old at a real transition.
    last_rate: AtomicU64,
    last_ratio: AtomicU64,
    /// Last readable buffer level — served when a reading lands
    /// mid-tick: nothing was taken, and the last level is the honest
    /// answer for what is sitting there.
    last_queued: AtomicUsize,
}

impl ConsoleAudio {
    fn player(&self) -> usize {
        self.player.load(Ordering::Relaxed)
    }
}

impl AudioDrain for ConsoleAudio {
    fn sample_rate(&self) -> f64 {
        if let Ok(mut link) = self.link.try_lock() {
            let rate = link.audio_sample_rate(self.player());
            // Zero also covers a core that reports no rate yet: either
            // way the resampler must not divide by it.
            if rate > 0.0 {
                self.last_rate.store(rate.to_bits(), Ordering::Relaxed);
                return rate;
            }
        }
        f64::from_bits(self.last_rate.load(Ordering::Relaxed))
    }

    fn framerate_ratio(&self, fps_target: f64) -> f64 {
        if let Ok(mut link) = self.link.try_lock() {
            let ratio = link.audio_framerate_ratio(self.player(), fps_target);
            self.last_ratio.store(ratio.to_bits(), Ordering::Relaxed);
            ratio
        } else {
            f64::from_bits(self.last_ratio.load(Ordering::Relaxed))
        }
    }

    fn drain(&mut self, out: &mut [i16]) -> Drained {
        let player = self.player();
        if let Ok(mut link) = self.link.try_lock() {
            let drained = link.drain_audio(player, out);
            self.last_queued.store(drained.queued, Ordering::Relaxed);
            drained
        } else {
            Drained {
                written: 0,
                queued: self.last_queued.load(Ordering::Relaxed),
            }
        }
    }
}
