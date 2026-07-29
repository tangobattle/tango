//! Where a console's sound comes out.
//!
//! The seam lives here — a console has to be able to answer "here is
//! my audio" in a vocabulary this crate can name — plus the one drain
//! implementation every engine shares, built on the [`Side`] audio
//! primitives. What a host then *does* with it — staging, resampling,
//! rate control — is a host's business and lives with the host.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::link::{Link, Side};

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

/// Where a drain reaches its console: one [`Side`] of whatever the
/// engine keeps behind its simulation lock. The seam's own sources are
/// a link's seat and a lone console; an engine with another shape
/// (mgba's replay pair) implements this over its own storage and
/// reuses the one drain via [`side_audio`].
pub trait SideSource: Send {
    /// Run `f` on the side, waiting out the simulation if it holds the
    /// console. Construction-time only — never off the sound callback.
    fn with_side(&self, f: &mut dyn FnMut(&mut dyn Side));

    /// Run `f` on the side only if the console is free right now;
    /// `false` if the simulation holds it.
    fn try_side(&self, f: &mut dyn FnMut(&mut dyn Side)) -> bool;
}

/// One console of a shared link, as the raw source a host resamples —
/// the drain every engine's match hands out. `player` is read per
/// fill, so a host that moves the sound between seats (training's side
/// swap) does so without the resampler above being rebuilt under it.
pub fn console_audio(link: Arc<Mutex<dyn Link>>, player: Arc<AtomicUsize>) -> Box<dyn AudioDrain> {
    side_audio(LinkSeat { link, player })
}

/// A lone console's audio, for the solo ride.
pub(crate) fn solo_audio(console: Arc<Mutex<dyn crate::Console>>) -> Box<dyn AudioDrain> {
    side_audio(LoneConsole { console })
}

/// The one drain implementation, over any [`SideSource`].
pub fn side_audio(source: impl SideSource + 'static) -> Box<dyn AudioDrain> {
    // Seed the rate fallback while the console is reachable:
    // construction runs off the sound callback, so a blocking wait is
    // fine here and never again after. The ratio starts at the native
    // 1.0 — there is no target rate to ask about yet.
    let mut rate = 0.0;
    source.with_side(&mut |side| rate = side.audio_sample_rate());
    Box::new(ConsoleAudio {
        source,
        last_rate: AtomicU64::new(rate.to_bits()),
        last_ratio: AtomicU64::new(1.0f64.to_bits()),
        last_queued: AtomicUsize::new(0),
    })
}

/// One seat of a link behind the engine's shared handle.
struct LinkSeat {
    link: Arc<Mutex<dyn Link>>,
    player: Arc<AtomicUsize>,
}

impl SideSource for LinkSeat {
    fn with_side(&self, f: &mut dyn FnMut(&mut dyn Side)) {
        let mut link = self.link.lock().unwrap();
        f(&mut *link.side(self.player.load(Ordering::Relaxed)));
    }

    fn try_side(&self, f: &mut dyn FnMut(&mut dyn Side)) -> bool {
        match self.link.try_lock() {
            Ok(mut link) => {
                f(&mut *link.side(self.player.load(Ordering::Relaxed)));
                true
            }
            Err(_) => false,
        }
    }
}

/// A console booted alone, behind the solo ride's lock.
struct LoneConsole {
    console: Arc<Mutex<dyn crate::Console>>,
}

impl SideSource for LoneConsole {
    fn with_side(&self, f: &mut dyn FnMut(&mut dyn Side)) {
        f(&mut *self.console.lock().unwrap().side());
    }

    fn try_side(&self, f: &mut dyn FnMut(&mut dyn Side)) -> bool {
        match self.console.try_lock() {
            Ok(mut console) => {
                f(&mut *console.side());
                true
            }
            Err(_) => false,
        }
    }
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
struct ConsoleAudio<S> {
    source: S,
    /// Last successfully read sample rate and framerate ratio, as f64
    /// bits — served whenever a read lands while the drive thread
    /// holds the console mid-tick. The stream re-reads both every
    /// fill, and it watches the ratio for the jump that means a
    /// deliberate speed change; a fast-forwarded drive loop holds the
    /// console for a large fraction of wall time, so a placeholder on
    /// those fills would read as the user toggling speed off and on
    /// again, snapping playback rate back and forth. A stale reading
    /// is only ever one fill old at a real transition.
    last_rate: AtomicU64,
    last_ratio: AtomicU64,
    /// Last readable buffer level — served when a reading lands
    /// mid-tick: nothing was taken, and the last level is the honest
    /// answer for what is sitting there.
    last_queued: AtomicUsize,
}

impl<S: SideSource> AudioDrain for ConsoleAudio<S> {
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

    fn framerate_ratio(&self, fps_target: f64) -> f64 {
        let mut ratio = 1.0;
        if self.source.try_side(&mut |side| ratio = side.audio_framerate_ratio(fps_target)) {
            self.last_ratio.store(ratio.to_bits(), Ordering::Relaxed);
            ratio
        } else {
            f64::from_bits(self.last_ratio.load(Ordering::Relaxed))
        }
    }

    fn drain(&mut self, out: &mut [i16]) -> Drained {
        let mut drained = Drained::default();
        if self.source.try_side(&mut |side| drained = side.drain_audio(out)) {
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
