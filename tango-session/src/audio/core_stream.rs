//! The audio stream, shared by every session kind: the host output
//! stream plays a live emulator core directly, pulling samples out of
//! its queue and resampling them to the device rate. This is dynamic
//! rate control (the same technique as mGBA's own frontend rate
//! control and RetroArch's DRC): the simulation is paced elsewhere —
//! the match clock (PvP), the playback drive loop (replay), a
//! wall-clock pacer (singleplayer) — and audio may never pause it, so
//! all regulation lives on the consumption side. The mechanisms:
//!
//! - Playback runs at the pace the console is *measured* to produce at.
//!   Samples are resampled from the console's own rate to the device's,
//!   scaled by that pace: a sim running at three-quarter speed
//!   (throttled for clock sync, or simply missing its target under a
//!   rollback storm) has three-quarters of the audio to play, so it is
//!   played over the same wall clock it took to make. That reads as
//!   pitch, which is deliberate: a pitch-preserving WSOLA stretcher
//!   used to take over past a hysteresis band, and its splices made
//!   fast-forward audio crunchy. A flat resample stays continuous —
//!   slowed or sped-up sound that still sounds like sound.
//!
//!   Measured, not declared, which is the whole point. The host
//!   publishes an fps *target*, but that is a command, not an outcome:
//!   under a rollback storm the sim misses it by tens of percent, and a
//!   stream that took the command at its word played tens of percent
//!   fast — audibly racing, with a draining queue behind it — while the
//!   tick rate sat rock steady. What the console actually handed over
//!   since the last fill is right there in the queue, so the stream
//!   counts it and believes that instead.
//!
//!   Fed forward, too, not servo'd toward. A loop closed on the queue
//!   level alone has to ring: level is the *integral* of the pace
//!   mismatch, so by the time it has moved enough to say "too slow" the
//!   rate has been wrong for a while, and the correction that follows
//!   overshoots by as much again. Pace is observable directly, so
//!   nothing has to be inferred from the level at all.
//! - The pace is smoothed, over about a second. Per-fill production is
//!   lumpy (a 60 Hz drive loop against a ~94 Hz callback hands some
//!   fills one tick and others two) and a PvP host's clock-sync
//!   throttler swings the sim's pace by whole fps within a second —
//!   corrective flutter around a near-constant mean, not a speed anyone
//!   chose. Followed raw, either would be playback lurching several
//!   percent back and forth: dragging and speeding. The queue absorbs
//!   them instead, which is what a queue is for, and pitch only glides.
//! - The level trim. Matching pace holds the queue only *neutrally* —
//!   production minus consumption still integrates into the level, so
//!   any perturbation (a stall, measurement bias, OS-clock-vs-DAC-
//!   crystal drift of tens to hundreds of ppm whose sign is random per
//!   machine) would re-base it permanently, ratcheting toward chronic
//!   underrun crackle or latency creep. A small trim on the played rate,
//!   against the level error, turns the target level into an attractor.
//! - The declared-speed snap. The one thing measurement is bad at is a
//!   step change, and a *deliberate* one — the user hitting
//!   fast-forward — is declared exactly and instantly by the host's fps
//!   target. So a large move there jumps the rate straight onto the new
//!   multiple, instead of smoothing its way over through a flooded or
//!   starved queue.
//! - The discard cap sheds, in one skip, a backlog only a producer
//!   burst can create — a replay seek chase, a perspective swap onto
//!   an undrained core's full ring, a device stall's catch-up.
//!   (Rollbacks can't get there: re-simulation replaces its revoked
//!   audio exactly.)
//! - Re-priming. A fully drained queue means the sim stalled; refilled
//!   at the trim's authority alone it would ride near-empty for
//!   seconds, where every jitter trough is an audible underrun. After
//!   a drain the stream serves silence until the queue reaches target
//!   once — one clean gap instead of seconds of crackle.
//!
//! The core access locks the same mutex the host's per-tick step
//! takes, so readout interleaves between ticks. A stalled sim
//! (reconnect pause, replay pause, a parked drive loop) still drains
//! the queue and goes silent — there's genuinely nothing to play.

use std::sync::atomic::Ordering;
use std::sync::Arc;

const EXPECTED_FPS: f32 = 60.0;

/// How much source audio to keep queued, in seconds — the level the trim
/// holds, and the floor on audio latency. Big enough to ride out
/// drive-thread/callback phase jitter, a couple of ticks of rollback
/// burst, and the excursion that smoothing the pace trades for smooth
/// pitch (see [`PACE_SMOOTHING_SECS`]).
const AUDIO_TARGET_QUEUED_SECS: f64 = 0.12;

/// Time constant, in seconds, of the average pace playback runs at.
/// Smoothing is needed because per-fill production is lumpy — a 60 Hz
/// drive loop against a ~94 Hz callback hands some fills a tick and
/// others two — and because a PvP host's clock-sync throttler swings
/// the sim's pace by whole fps within a second, corrective flutter
/// around a near-constant mean rather than a speed anyone chose.
///
/// What smoothing costs the queue is the pace error times *how long it
/// lasts*, capped at this: flutter reverses within a few hundred ms and
/// so costs tens of ms of level, while a change that sticks is followed
/// within about this long. Both fit inside
/// [`AUDIO_TARGET_QUEUED_SECS`], which is how the constants are paired.
const PACE_SMOOTHING_SECS: f64 = 1.0;

/// Bounds on measured pace, as multiples of the console's own rate — a
/// guard on the arithmetic (a stalled sim measures zero), wide enough to
/// clear any speed a host actually paces at.
const PACE_BOUNDS: std::ops::RangeInclusive<f64> = 0.1..=10.0;

/// How fast the played rate may move toward the measured pace, as a
/// fraction of itself per second, when the queue is sitting where it
/// should. Deliberately a crawl. No estimator can smooth away what a
/// rollback does to short-term pace: the catch-up burst lands several
/// fills of audio at once and the lull that balances it has not happened
/// yet, so *any* average briefly reads high. Measured pace is therefore
/// where playback is headed, not where it goes this instant, and the
/// queue carries the difference — which is exactly the queue's job.
const RATE_SLEW_CALM_PER_SEC: f64 = 0.005;

/// Extra slew allowed at a full-target level error, squared in between.
/// The level is what says whether a pace change was a blip worth
/// ignoring or something real: a burst the queue swallows never moves it
/// much, while a sim that has genuinely dropped to three-quarter speed
/// drags it down and keeps going. So urgency is read off the level, and
/// pitch hurries only when the alternative is running the queue dry.
const RATE_SLEW_URGENT_PER_SEC: f64 = 0.5;

/// The level servo's authority: the largest fractional trim it may add
/// to the measured pace. Matching pace holds the level only *neutrally*
/// — production minus consumption still integrates into it, so any
/// perturbation (a stall, a measurement bias, OS-clock-vs-DAC-crystal
/// drift of tens to hundreds of ppm whose sign is random per machine)
/// would re-base it permanently, ratcheting toward chronic underrun
/// crackle or latency creep. This is what makes the target an attractor
/// instead. ±2% is ~34 cents at the very edge, it is reached only at a
/// full-target error, and it moves no faster than the level does — a
/// drift, never a warble.
const AUDIO_MAX_TRIM: f64 = 0.02;

/// Fractional move in the host's declared clock that counts as a
/// deliberate speed change rather than pacing noise, and so jumps the
/// played rate straight to the new multiple. Clock-sync shaves are whole fps
/// out of 60 and stay well under this; fast-forward clears it in one
/// hop, and smoothing into one would flood or starve the queue for the
/// whole climb.
const SPEED_SNAP_DEV: f64 = 0.15;

/// Queue level, as a multiple of the target, past which `fill` stops
/// absorbing and discards the oldest samples back down to target.
/// Healthy operation never exceeds ~1.7x (target plus two ticks of
/// phase swing), and rollbacks can't get here (re-simulation replaces
/// its revoked audio exactly) — only producer bursts do: a replay seek
/// chase, a perspective swap onto an undrained core's full ring, a
/// device stall's catch-up. One skip beats seconds of extra latency
/// (the trim alone would need ~5 s per 100 ms of backlog).
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
    /// Smoothed pace the console is actually producing at, as a multiple
    /// of its own rate — where playback is headed. Zero until the first
    /// fill seeds it.
    pace: f64,
    /// The rate actually played, as a multiple of the console's own —
    /// what pitch rides on. Slew-limited toward `pace`.
    played: f64,
    /// Queue level this fill's resampling should have left behind, so
    /// the next one can tell production from what it consumed itself.
    /// `None` before there is a baseline to measure against.
    expected: Option<f64>,
    /// Last clock the host declared, to spot a deliberate speed change
    /// against ([`SPEED_SNAP_DEV`]).
    declared: f64,
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
            pace: 0.0,
            played: 0.0,
            expected: None,
            declared: 0.0,
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

        // A deliberate speed change is the one thing measurement is bad
        // at — a step, where measuring means lagging — and the one thing
        // the host declares exactly: jump straight onto the new
        // multiple. Everything smaller (clock-sync shaves, a sim missing
        // its target) is left to the measurement, which sees what
        // actually happened rather than what was asked for.
        let declared = self.pull.framerate_ratio(fps_target as f64);
        if declared > 0.0 {
            if self.pace <= 0.0 || (declared - self.declared).abs() > self.declared * SPEED_SNAP_DEV {
                self.pace = 1.0 / declared;
                self.played = self.pace;
                // The jump invalidates the baseline: the level it was
                // taken against was consumed at the old rate.
                self.expected = None;
            }
            self.declared = declared;
        }

        let target = source_rate * AUDIO_TARGET_QUEUED_SECS;
        let queued = self.pull.source_available() as f64;

        // Pace: what the console handed over since the last fill,
        // against what one fill at 1x would be. This is the measurement
        // the whole stream turns on — the sim's real speed, whatever the
        // host meant to pace it at — and it is fed forward rather than
        // servo'd toward, because a loop closed on the queue level alone
        // has to ring: level is an integral of the mismatch, so by the
        // time the level says "too slow" the rate has already been wrong
        // for a while, and the correction overshoots by as much again.
        //
        // Signed, and that matters more than it looks. A rollback takes
        // speculated audio back out of the queue before re-simulation
        // puts it back, so across a mispredict production arrives as a
        // dip and then a burst that cancel exactly. Floor the dip at
        // zero and they stop cancelling: every mispredict nets positive,
        // pace ratchets up, and playback speeds up on every rollback —
        // which is precisely what a rectified measurement sounds like.
        let secs = frame_count as f64 / self.out_rate as f64;
        if let Some(expected) = self.expected {
            let produced = (queued - expected) / (source_rate * secs);
            self.pace = (self.pace + (secs / PACE_SMOOTHING_SECS).min(1.0) * (produced - self.pace))
                .clamp(*PACE_BOUNDS.start(), *PACE_BOUNDS.end());
        }

        if queued > target * AUDIO_DISCARD_FACTOR {
            // Producer-burst backlog (seek chase, perspective swap,
            // device-stall catch-up): skip the oldest samples in one go
            // rather than carrying seconds of extra latency.
            self.pull.discard_source((queued - target) as usize);
        }

        // Re-prime across stalls: hold silence until the queue is back
        // at target, instead of riding near-empty at the servo's slow
        // refill.
        let queued = self.pull.source_available() as f64;
        if self.priming {
            if queued < target {
                // Silence until the queue rebuilds. Nothing was
                // consumed, so all of it counts as already produced.
                self.expected = Some(queued);
                return 0;
            }
            self.priming = false;
        }

        // Head for the measured pace, trimmed toward the target level —
        // the trim is what makes the target an attractor rather than
        // wherever the level happened to drift to — but get there no
        // faster than the queue says is necessary. Calm queue, crawl;
        // draining queue, hurry.
        let error = ((queued - target) / target).clamp(-1.0, 1.0);
        let want = self.pace * (1.0 + AUDIO_MAX_TRIM * error);
        let slew = self.played * secs * (RATE_SLEW_CALM_PER_SEC + RATE_SLEW_URGENT_PER_SEC * error * error);
        self.played += (want - self.played).clamp(-slew, slew);
        let rate = self.played;

        // Claiming the source faster than it really is makes each output
        // frame eat more of it — which is playing it faster, and higher.
        // Pitch is the artifact; continuity is what it buys.
        //
        // Only this fill's worth is resampled. Everything past it stays
        // in the source queue, which is what `source_available` measures
        // and therefore the only queue the servo can steer.
        self.pull
            .process(source_rate * rate, self.out_rate as f64, frame_count);
        // Where the next fill starts counting production from: this
        // fill's own reading, less what it just consumed. Derived rather
        // than read back — reading it again would be a second reach into
        // the console, and the sound callback shares a lock with the
        // simulation, so every reach is a chance to stall behind a tick.
        // It would also be a second sample point, taken at a different
        // moment than the one the level error came from, which puts
        // noise straight into the pace the ear hears.
        let consumed = frame_count as f64 * source_rate * rate / self.out_rate as f64;
        self.expected = Some((queued - consumed).max(0.0));

        let delivered = self.pull.available().min(frame_count);
        let linear_buf: &mut [i16] = bytemuck::cast_slice_mut(buf);
        self.pull
            .read(&mut linear_buf[..delivered * super::NUM_CHANNELS], delivered);

        // A fill with nothing at all to give is the stall signature —
        // and the moment an artifact was unavoidable anyway. Latch
        // priming so recovery is one clean gap instead of seconds of
        // jitter-trough crackle at a near-empty queue.
        //
        // Nothing at all, not merely short. Re-priming answers a
        // shortfall with a queue-target's worth of silence, which is a
        // fine trade against a stalled sim and a terrible one against a
        // fill that came up a few frames light — that turns a click into
        // a gap a hundred times its length.
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
                capacity: 1 << 20,
            }
        }
    }

    /// One run's observations: the queue level the servo saw on each
    /// fill (in source frames — the audio latency), how many fills the
    /// queues could not cover, and the playback rate applied.
    struct Run {
        queued: Vec<f64>,
        short: usize,
        rate: Vec<f64>,
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
            rate: Vec::new(),
        };
        for i in 0..fills {
            let fps = fps(i);
            published.store((fps as f32).to_bits(), Ordering::Relaxed);
            // A sim paced at `fps` produces that fraction of a native
            // second's audio per second of wall clock.
            deliver(i, RATE * secs_per_fill * fps / NATIVE_FPS, &ring);
            if <CoreStream as Stream>::fill(&mut stream, &mut buf) < FILL {
                run.short += 1;
            }
            // The unresampled backlog, which is both the audio latency
            // and the level the servo steers. Reading it drains the ring
            // exactly the way the next fill would, so sampling it here
            // costs the run nothing.
            run.queued.push(stream.pull.source_available() as f64);
            // Playback rate as a multiple of the console's own — what
            // pitch rides on.
            run.rate.push(stream.played);
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
        assert_settled(&run, 0.4, 1.7);
        // And the burst must not reach pitch. A catch-up lands several
        // fills of audio at once, so every average of pace briefly reads
        // high; played straight through, that is playback speeding up on
        // every mispredict — audible, and the report this was written
        // for. The queue is supposed to swallow it whole.
        let settled = &run.rate[run.rate.len() / 2..];
        let lo = settled.iter().copied().fold(f64::MAX, f64::min);
        let hi = settled.iter().copied().fold(f64::MIN, f64::max);
        assert!(
            hi - lo < 0.005,
            "rollback bursts moved playback {lo:.4}..{hi:.4} ({:.1}%)",
            (hi - lo) * 100.0
        );
    }

    /// The report this was written for: with rollback forced on, the sim
    /// misses the fps target the host keeps publishing — by a lot, and
    /// steadily. A stream that believed the published target would play
    /// at full rate against three-quarter-rate production: audibly
    /// racing, the queue draining behind it into repeated re-priming
    /// silences, all while the tick rate sat rock steady. The servo has
    /// to find the real pace from the queue alone, and hold it.
    #[test]
    fn a_sim_that_misses_its_target_does_not_race() {
        // The host still asks for full speed; the console only manages
        // three-quarters of it.
        let run = run(6000, native, |_, produced, ring| {
            *ring.lock().unwrap() += produced * 0.75;
        });
        for i in (0..run.queued.len()).step_by(300) {
            println!("fill {i:5} queued={:8.0} rate={:.5}", run.queued[i], run.rate[i]);
        }
        assert_settled(&run, 0.6, 1.5);
        // Rate has to converge on the pace that is actually there.
        let settled = &run.rate[run.rate.len() / 2..];
        for rate in settled {
            assert!(
                (rate - 0.75).abs() < 0.02,
                "playback settled at {rate:.4}x against 0.75x production"
            );
        }
        // Only the fills before it converged may go short.
        assert!(
            run.short < 400,
            "{} fills went short chasing the real pace",
            run.short
        );
    }

    /// Hitting fast-forward is a step change, and measuring a step means
    /// lagging it: the queue fills at four times the rate it drains for
    /// however long the lag lasts, and past the discard cap that is a
    /// skip. The host declares this one exactly, so it is taken at its
    /// word — worth the few lines. Measured here with the snap disabled,
    /// the same 4x took 4.1 s to settle and peaked at 0.96x the discard
    /// cap: no skip, but only just, and with four seconds of audible
    /// glide getting there. With it, immediate and 0.31x.
    #[test]
    fn fast_forward_lands_without_a_flood() {
        // 4x from fill 1000 on, production following.
        let speed = |i: usize| if i < 1000 { NATIVE_FPS } else { NATIVE_FPS * 4.0 };
        let run = run(3000, speed, promptly);
        let target = RATE * AUDIO_TARGET_QUEUED_SECS;

        assert_eq!(run.short, 0, "fast-forward starved a fill");
        let peak = run.queued[1000..].iter().copied().fold(0.0, f64::max);
        assert!(
            peak < target * AUDIO_DISCARD_FACTOR * 0.5,
            "queue piled up to {peak:.0} source frames, half the discard cap being {:.0}",
            target * AUDIO_DISCARD_FACTOR * 0.5
        );
        // On the new speed within a few fills, and steady there.
        for rate in &run.rate[1010..] {
            assert!((rate - 4.0).abs() < 0.1, "playback ran at {rate:.3}x against 4x");
        }
    }

    /// A PvP host restates its fps target every tick and the clock-sync
    /// throttler swings it by whole fps well inside a second. Followed
    /// closely that lands on playback rate — audibly dragging and
    /// speeding while the tick rate looks steady — so the servo has to
    /// crawl near target and let the queue absorb the swing.
    #[test]
    fn clock_sync_flutter_does_not_reach_the_playback_rate() {
        // A 3 fps shave, on and off every ~0.8 s — the throttler's
        // fast-fall EMA moves at about that.
        let flutter = |i: usize| NATIVE_FPS - if (i / 36) % 2 == 0 { 3.0 } else { 0.0 };
        let run = run(6000, flutter, promptly);
        assert_eq!(run.short, 0, "clock-sync flutter starved a fill");
        assert_settled(&run, 0.6, 1.5);
        // Peak-to-peak playback rate over the settled span. The raw
        // shave is 5% — nearly a semitone; smoothed it has to land far
        // under that, or it is still a warble.
        let settled = &run.rate[run.rate.len() / 2..];
        let lo = settled.iter().copied().fold(f64::MAX, f64::min);
        let hi = settled.iter().copied().fold(f64::MIN, f64::max);
        assert!(
            hi - lo < 0.01,
            "playback rate swung {lo:.4}..{hi:.4} ({:.1}%), still a warble",
            (hi - lo) * 100.0
        );
    }
}
