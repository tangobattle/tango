//! The audio stream, shared by every session kind: the host output
//! stream plays a live emulator core directly, pulling samples out of
//! its queue and resampling them to the device rate. This is dynamic
//! rate control (the same technique as mGBA's own frontend rate
//! control and RetroArch's DRC): the simulation is paced elsewhere —
//! the match clock (PvP), the playback drive loop (replay), a
//! wall-clock pacer (singleplayer) — and audio may never pause it, so
//! all regulation lives on the consumption side. The mechanisms:
//!
//! - The faux clock scales nominal consumption to the host's published
//!   fps target (read through a closure, since the host drive loop owns
//!   pacing): a throttled sim stretches playback by the same ratio and
//!   a fast-forwarded one compresses into it, instead of starving or
//!   flooding the device. It folds into the resample ratio, so it reads
//!   as pitch — inaudible at the GBA's 59.7275 Hz against a 60 fps
//!   target, chipmunk or drone at a real speed multiple. That is
//!   deliberate: a pitch-preserving WSOLA stretcher used to take over
//!   past a hysteresis band, and its splices made fast-forward audio
//!   crunchy. A flat resample stays continuous — sped-up sound that
//!   still sounds like sound.
//! - The occupancy servo. A bare rate match leaves the queue level only
//!   neutrally stable — production minus consumption integrates into
//!   the level, so any perturbation (a stall draining the queue, a
//!   drive-loop cadence resync, OS-clock-vs-DAC-crystal drift of tens
//!   to hundreds of ppm whose sign is random per machine) would
//!   permanently re-base it, ratcheting toward chronic underrun crackle
//!   or latency creep. Trimming the claimed source rate against the
//!   level error turns the target level into an attractor.
//! - The discard cap sheds, in one skip, a backlog only a producer
//!   burst can create — a replay seek chase, a perspective swap onto an
//!   undrained core's full ring, a device stall's catch-up.
//! - Re-priming. A fully drained queue means the sim stalled; refilled
//!   at the servo's authority alone it would ride near-empty for ~10
//!   seconds, where every jitter trough is an audible underrun. After a
//!   drain the stream serves silence until the queue reaches target
//!   once — one clean ~50 ms gap instead of seconds of crackle.
//!
//! The level the servo steers is the *unresampled* queue, which is why
//! [`Resampler`](super::Resampler) converts only what each fill plays:
//! resampling everything queued would put the backlog downstream of the
//! only thing regulating it.
//!
//! The core access locks the same mutex the host's per-tick step takes,
//! so readout interleaves between ticks. A stalled sim (reconnect
//! pause, replay pause, a parked drive loop) still drains the queue and
//! goes silent — there's genuinely nothing to play.

use std::sync::atomic::Ordering;
use std::sync::Arc;

const EXPECTED_FPS: f32 = 60.0;

/// How much source audio to keep queued in the played core's sample
/// buffer, in seconds — the level the servo holds, and the floor on
/// audio latency. Big enough to ride out drive-thread/callback phase
/// jitter and a couple of ticks of rollback burst.
const AUDIO_TARGET_QUEUED_SECS: f64 = 0.05;

/// The occupancy servo's authority: the largest fractional trim it may
/// put on the resample ratio. ±0.5% is ~9 cents of pitch — inaudible —
/// yet 10-100x the clock drift it has to cancel, recentering the queue
/// at up to ~5 ms per second (a half-target error in a few seconds).
const AUDIO_MAX_TRIM: f64 = 0.005;

/// Queue level, as a multiple of the target, past which `fill` stops
/// absorbing and discards the oldest samples back down to target.
/// Healthy operation never exceeds ~1.7x (target plus two ticks of
/// phase swing) — only producer bursts get here: a replay seek chase, a
/// perspective swap onto an undrained core's full ring, a device
/// stall's catch-up. One skip beats seconds of extra latency (the servo
/// alone would need ~10 s per 50 ms of backlog).
const AUDIO_DISCARD_FACTOR: f64 = 3.0;

pub struct CoreStream {
    /// The console's audio, already resampled to the device rate by
    /// whichever backend produced it. This stream never learns which
    /// emulator that was.
    pull: super::Resampler,
    /// The host drive loop's current pacing target, f32. Zero or less is
    /// treated as unthrottled (60 fps).
    fps_target: Box<dyn Fn() -> f32 + Send>,
    out_rate: u32,
    /// Serve silence until the queue reaches target once. Latched at
    /// construction (the session starts with an empty queue) and again
    /// whenever the queue fully drains — the signature of a stalled sim.
    /// Without it, the post-stall rebuild happens at the trim's few ms of
    /// queue per second and the stream rides near-empty for seconds,
    /// crackling on every jitter trough; one clean gap beats that.
    /// (The wall-clock-era reservoir learned this; the mechanism was lost
    /// in the revert.)
    priming: bool,
}

impl CoreStream {
    pub fn new(
        pull: Box<dyn tango_match::AudioDrain>,
        fps_target: impl Fn() -> f32 + Send + 'static,
        out_rate: u32,
    ) -> Self {
        Self {
            pull: super::Resampler::new(pull),
            fps_target: Box::new(fps_target),
            out_rate: if out_rate == 0 { 48000 } else { out_rate },
            priming: true,
        }
    }

    /// A `fps_target` closure over an `f32`-bits atomic — the shape most
    /// hosts publish their pacing through.
    pub fn fps_from_bits(bits: Arc<std::sync::atomic::AtomicU32>) -> impl Fn() -> f32 + Send + 'static {
        move || f32::from_bits(bits.load(Ordering::Relaxed))
    }
}

impl super::Stream for CoreStream {
    fn fill(&mut self, buf: &mut [[i16; super::NUM_CHANNELS]]) -> usize {
        let frame_count = buf.len();

        let mut fps_target = (self.fps_target)();
        if fps_target <= 0.0 {
            fps_target = EXPECTED_FPS;
        }

        // The console's production rate can change at runtime (BN4+
        // flip from 32768 to 65536 Hz after boot), so it is re-read
        // every fill.
        let source_rate = self.pull.sample_rate();

        // The faux clock: production scales with the sim's pace, so a
        // throttled sim stretches playback by the same ratio instead of
        // starving it (and a fast-forwarded one compresses).
        let faux_clock = self.pull.framerate_ratio(fps_target as f64);

        let target = source_rate * AUDIO_TARGET_QUEUED_SECS;
        let mut queued = self.pull.source_available() as f64;
        if queued > target * AUDIO_DISCARD_FACTOR {
            // Producer-burst backlog (seek chase, perspective swap,
            // device-stall catch-up): skip the oldest samples in one go
            // rather than carrying seconds of extra latency.
            self.pull.discard_source((queued - target) as usize);
            queued = target;
        }

        // Re-prime across stalls: hold silence until the queue is back
        // at target, instead of riding near-empty at the servo's slow
        // refill.
        if self.priming {
            if queued < target {
                return 0;
            }
            self.priming = false;
        }

        // Servo: nudge the claimed source rate so the queue level
        // converges on the target. Claiming the source faster than it is
        // makes each output frame consume more of it (drains); slower,
        // less (refills).
        let trim = AUDIO_MAX_TRIM * ((queued - target) / target).clamp(-1.0, 1.0);
        // The faux clock folds into the destination claim: a
        // fast-forwarded sim asks for fewer device frames per second of
        // production, which plays it back faster (and higher). Pitch is
        // the artifact; continuity is what it buys.
        self.pull.process(
            source_rate * (1.0 + trim),
            self.out_rate as f64 * faux_clock,
            frame_count,
        );

        let delivered = self.pull.available().min(frame_count);
        let linear_buf: &mut [i16] = bytemuck::cast_slice_mut(buf);
        self.pull
            .read(&mut linear_buf[..delivered * super::NUM_CHANNELS], delivered);

        // A fill with nothing at all to give is the stall signature —
        // and the moment an artifact was unavoidable anyway. Latch
        // priming so recovery is one clean gap instead of seconds of
        // jitter-trough crackle at a near-empty queue. (Nothing at all,
        // not merely short: re-priming answers with a whole target's
        // worth of silence, which is a fine trade against a stalled sim
        // and a terrible one against a fill that came up a few frames
        // light.)
        if delivered == 0 {
            self.priming = true;
        }
        delivered
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::super::{Stream, NUM_CHANNELS};
    use super::*;

    const RATE: f64 = 32768.0;
    const OUT_RATE: u32 = 48000;
    const FILL: usize = 512;

    /// A console ring the test produces into by hand.
    struct Ring(Arc<Mutex<f64>>);

    impl tango_match::AudioDrain for Ring {
        fn sample_rate(&self) -> f64 {
            RATE
        }

        /// How long a second of this console's production lasts once the
        /// host paces it at `fps_target` — mgba's convention.
        fn framerate_ratio(&self, fps_target: f64) -> f64 {
            if fps_target > 0.0 {
                NATIVE_FPS / fps_target
            } else {
                1.0
            }
        }

        fn drain(&mut self, out: &mut [i16]) -> tango_match::Drained {
            let mut level = self.0.lock().unwrap();
            let written = (*level as usize).min(out.len() / 2);
            *level -= written as f64;
            out[..written * 2].fill(1);
            tango_match::Drained {
                written,
                queued: *level as usize,
            }
        }
    }

    /// One run's observations: the queue level the servo saw on each
    /// fill (in source frames — the audio latency), and how many fills
    /// the queues could not cover.
    struct Run {
        queued: Vec<f64>,
        short: usize,
    }

    /// GBA fps: what an unthrottled host publishes, and the pace a
    /// console's production is measured against.
    const NATIVE_FPS: f64 = 59.7275;

    /// Drive `fills` device callbacks against a console producing in
    /// real time. `fps` is what the host publishes as its pacing target
    /// on each fill (the console's production follows it, since the
    /// drive loop hits its target); `deliver` decides *when* that
    /// production reaches the ring.
    fn run(fills: usize, fps: impl Fn(usize) -> f64, mut deliver: impl FnMut(usize, f64, &Mutex<f64>)) -> Run {
        let ring = Arc::new(Mutex::new(RATE * AUDIO_TARGET_QUEUED_SECS));
        let pull = Box::new(Ring(ring.clone()));
        let published = Arc::new(std::sync::atomic::AtomicU32::new((NATIVE_FPS as f32).to_bits()));
        let mut stream = CoreStream::new(pull, CoreStream::fps_from_bits(published.clone()), OUT_RATE);

        let secs_per_fill = FILL as f64 / OUT_RATE as f64;
        let mut buf = vec![[0i16; NUM_CHANNELS]; FILL];
        let mut run = Run {
            queued: Vec::new(),
            short: 0,
        };
        for i in 0..fills {
            let fps = fps(i);
            published.store((fps as f32).to_bits(), Ordering::Relaxed);
            // A sim paced at `fps` produces that fraction of a native
            // second's audio per second of wall clock.
            deliver(i, RATE * secs_per_fill * fps / NATIVE_FPS, &ring);
            // The unresampled backlog: the audio latency, and the level
            // the servo steers. Sampled where the servo itself sees it —
            // before this fill consumes — since after would read a
            // fill's worth low and make a settled queue look starved.
            run.queued.push(stream.pull.source_available() as f64);
            if <CoreStream as Stream>::fill(&mut stream, &mut buf) < FILL {
                run.short += 1;
            }
        }
        run
    }

    fn native(_: usize) -> f64 {
        NATIVE_FPS
    }

    /// The plain case: everything the console produced lands in the ring
    /// on the fill that produced it.
    fn promptly(_: usize, produced: f64, ring: &Mutex<f64>) {
        *ring.lock().unwrap() += produced;
    }

    /// The servo holds the queue at its target. Resampling the whole
    /// source queue per fill instead of just the fill's worth used to
    /// move the backlog downstream of the level the servo measures: the
    /// trim then sat pinned at full authority against a level it could
    /// not move, latency crept up by ~4 ms per second, and once the
    /// resampled side hit its cap every fill shed the oldest samples it
    /// held — continuous, audible skipping.
    #[test]
    fn the_queue_settles_at_the_target_instead_of_creeping() {
        let run = run(6000, native, promptly);
        assert_eq!(run.short, 0, "a fill went short at steady state");
        assert_settled(&run, 0.8, 1.25);
    }

    /// The queue stays inside `[lo, hi]` multiples of the target once
    /// settled — it neither creeps upward (latency, then shedding) nor
    /// walks down toward the underruns that latch re-priming.
    fn assert_settled(run: &Run, lo: f64, hi: f64) {
        let target = RATE * AUDIO_TARGET_QUEUED_SECS;
        let settled = &run.queued[run.queued.len() / 2..];
        let low = settled.iter().copied().fold(f64::MAX, f64::min);
        let high = settled.iter().copied().fold(f64::MIN, f64::max);
        assert!(
            low > target * lo && high < target * hi,
            "queue ranged {low:.0}..{high:.0} source frames, target {target:.0}"
        );
    }

    /// A rollback delivers its audio lumpily: the speculative span is
    /// pulled early, then the ring gets nothing while re-simulation
    /// regenerates what already played and the engine swallows it. The
    /// stream has to ride that out of its queue — no shortfall (which
    /// latches re-priming, and that is a ~50 ms silence) and no
    /// discard-cap skip.
    #[test]
    fn a_rollback_burst_does_not_break_the_stream() {
        let run = run(6000, native, |i, produced, ring| {
            let phase = i % 40;
            // The next four fills' audio is already in the ring,
            // speculated; while it is re-simulated and swallowed the
            // ring gets nothing.
            if (20..24).contains(&phase) {
                return;
            }
            let speculated = if phase == 19 { 4.0 * produced } else { 0.0 };
            *ring.lock().unwrap() += produced + speculated;
        });
        assert_eq!(run.short, 0, "a rollback burst starved a fill");
        // Absorbed, not shed: the queue swings with the burst but never
        // reaches the cap, which would be an audible skip.
        assert_settled(&run, 0.4, AUDIO_DISCARD_FACTOR);
    }


    /// Fast-forward is a step change in how fast the console produces,
    /// and the faux clock carries it straight through: the queue must
    /// not pile up on the way, which past the discard cap would be a
    /// skip.
    #[test]
    fn fast_forward_lands_without_a_flood() {
        // 4x from fill 1000 on, production following.
        let speed = |i: usize| if i < 1000 { NATIVE_FPS } else { NATIVE_FPS * 4.0 };
        let run = run(3000, speed, promptly);
        let target = RATE * AUDIO_TARGET_QUEUED_SECS;

        assert_eq!(run.short, 0, "fast-forward starved a fill");
        let peak = run.queued[1000..].iter().copied().fold(0.0, f64::max);
        assert!(
            peak < target * AUDIO_DISCARD_FACTOR,
            "queue piled up to {peak:.0} source frames, past a discard cap of {:.0}",
            target * AUDIO_DISCARD_FACTOR
        );
    }

}
