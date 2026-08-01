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
//!   once — one clean ~50 ms gap instead of seconds of crackle. A sim
//!   that can't hold its published pace (DS-class load) never fully
//!   drains — it starves every fill a little instead — so a few starved
//!   fills in a row concede the same way.
//!
//! The level the servo steers is the *unresampled* queue, which is why
//! [`Resampler`](super::Resampler) converts only what each fill plays:
//! resampling everything queued would put the backlog downstream of the
//! only thing regulating it.
//!
//! The core access locks the same mutex the host's per-tick step takes,
//! so readout interleaves between ticks. That interleaving is enough
//! only while the lock is mostly free: a drive loop running at its
//! wall-clock budget — 4x fast-forward, DS-class tick costs — holds it
//! for essentially all of every period, and the fill's own try-lock
//! then lands essentially never, starving the stream with whole
//! seconds of production sitting unreachable behind the lock. The
//! [`Intake`] handle closes that: the drive loop pumps it once per
//! tick, right after its own step returns — the one moment the lock is
//! known to be free — so the reach lands there no matter how saturated
//! the loop is, and the fill only ever reads what is already staged.
//! Sessions with rollback don't pump (revocation depends on
//! speculative samples staying in the console's ring as long as
//! possible); they run at 1x, where the fill's own reach suffices. A
//! stalled sim (reconnect pause, replay pause, a parked drive loop)
//! still drains the queue and goes silent — there's genuinely nothing
//! to play.

use std::sync::atomic::Ordering;
use std::sync::Arc;

/// How much audio to keep queued in the played core's sample buffer,
/// in seconds of *playback* — the level the servo holds, and the floor
/// on audio latency. Big enough to ride out drive-thread/callback
/// phase jitter and a couple of ticks of rollback burst.
///
/// Playback seconds, not source seconds: at 4x the device consumes
/// source four times as fast, so holding the same playback margin
/// takes four times the source frames. Sized in source seconds alone
/// the queue thins from ~4.7 fills deep to ~1.2 at 4x — every jitter
/// trough a short fill, fast-forward audibly dropping audio.
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

/// Consecutive starved fills — short, with the queue genuinely low —
/// that latch re-priming. Sustained starvation is the signature of a
/// sim that can't hold its published pace (DS-class load): it starves
/// every fill a little, which the total-stall latch below never sees.
/// Three fills is ~30 ms of continuous artifact before conceding; a
/// one-off delivery hiccup recovers on the next fill and resets the
/// count instead.
const STARVED_FILLS_TO_REPRIME: u32 = 3;

pub struct Stream {
    /// The console's audio, already resampled to the device rate by
    /// whichever backend produced it. This stream never learns which
    /// emulator that was. Shared with the [`Intake`] the drive loop
    /// pumps; both holds are bounded (a fill's worth of resampling, a
    /// take's worth of memcpy), so neither side can stall the other
    /// past that.
    pull: Arc<std::sync::Mutex<super::Resampler>>,
    /// The console's own frame rate
    /// ([`FrameTiming::fps`](tango_match::FrameTiming::fps)) — the pace
    /// its audio production is a function of, and so what the faux
    /// clock below measures the host's target against.
    expected_fps: f32,
    /// The host drive loop's current pacing target, f32. Zero or less is
    /// treated as the console's own rate.
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
    /// The previous fill's queue target, in source frames — a jump in it
    /// (fast-forward engaging, a console's rate flip) re-latches priming,
    /// since the servo alone would take seconds to deepen the queue and
    /// ride thin the whole way there.
    last_target: f64,
    /// Consecutive fills that came up short with the queue below target
    /// — the sustained-starvation detector behind the partial-deficit
    /// re-prime (see [`STARVED_FILLS_TO_REPRIME`]).
    starved_fills: u32,
}

impl Stream {
    pub fn new(
        pull: Box<dyn super::Drain>,
        expected_fps: f32,
        fps_target: impl Fn() -> f32 + Send + 'static,
        out_rate: u32,
    ) -> Self {
        Self {
            pull: Arc::new(std::sync::Mutex::new(super::Resampler::new(pull))),
            expected_fps,
            fps_target: Box::new(fps_target),
            out_rate: if out_rate == 0 { 48000 } else { out_rate },
            priming: true,
            last_target: f64::INFINITY,
            starved_fills: 0,
        }
    }

    /// A `fps_target` closure over an `f32`-bits atomic — the shape most
    /// hosts publish their pacing through.
    pub fn fps_from_bits(bits: Arc<std::sync::atomic::AtomicU32>) -> impl Fn() -> f32 + Send + 'static {
        move || f32::from_bits(bits.load(Ordering::Relaxed))
    }

    /// The stream's intake, for the thread that drives the simulation
    /// (see [`Intake`]).
    pub fn intake(&self) -> Intake {
        Intake(self.pull.clone())
    }
}

/// The stream's intake, held by whoever turns the simulation's crank.
///
/// The fill reaches into the console with a try-lock, which lands only
/// while the console's lock is mostly free. A drive loop running at
/// its wall-clock budget — 4x fast-forward, DS-class tick costs —
/// holds that lock for essentially all of every period, so the fill's
/// reach starves against exactly the sessions that produce the most
/// audio. The drive loop itself has what no other thread has: a moment
/// per tick, right after its own step returns, where the console's
/// lock is known to be free. [`pump`](Intake::pump) called there moves
/// the console's ring into the stream's staging through the same drain
/// the fill uses, and never contends with anything for longer than a
/// take's worth of memcpy.
///
/// Optional: a session whose drive loop never saturates (PvP's match
/// clock at 1x) plays fine without pumping — and a rollback session
/// must not pump, since revoking mispredicted audio depends on
/// speculative samples staying in the console's own ring for as long
/// as possible.
#[derive(Clone)]
pub struct Intake(Arc<std::sync::Mutex<super::Resampler>>);

impl Intake {
    /// Move what the console has produced into the stream's staging —
    /// call once per tick, after stepping the simulation.
    pub fn pump(&self) {
        self.0.lock().unwrap().source_available();
    }
}

impl super::Source for Stream {
    fn fill(&mut self, buf: &mut [[i16; super::NUM_CHANNELS]]) -> usize {
        let frame_count = buf.len();
        // Shared only with the intake's bounded memcpy-sized holds, so
        // this is not the unbounded simulation-lock wait the fill must
        // never take.
        let mut pull = self.pull.lock().unwrap();

        let mut fps_target = (self.fps_target)();
        if fps_target <= 0.0 {
            fps_target = self.expected_fps;
        }

        // The console's production rate can change at runtime (BN4+
        // flip from 32768 to 65536 Hz after boot), so it is re-read
        // every fill.
        let source_rate = pull.sample_rate();

        // The faux clock: production scales with the sim's pace, so a
        // throttled sim stretches playback by the same ratio instead of
        // starving it (and a fast-forwarded one compresses).
        //
        // Native over target, *not* the other way round: at twice the
        // console's framerate it emits twice the audio per wall-clock
        // second, so what it produced in a second now has to play in
        // half of one and the ratio falls. It scales the resampler's
        // destination rate directly, so inverting it would make a
        // fast-forward play *slower* rather than faster.
        //
        // Arithmetic on the console's own frame clock rather than a
        // question for the console: production per frame is fixed, and
        // both terms are already here. Asking meant a lock-guarded read
        // per fill, with a cached answer for the fills that missed —
        // and a fast-forwarded drive loop holds the console for most of
        // wall time, so those were exactly the fills a speed change
        // landed on.
        let mut faux_clock = self.expected_fps as f64 / fps_target as f64;
        if !faux_clock.is_finite() || faux_clock <= 0.0 {
            faux_clock = 1.0;
        }

        // The faux clock folds into the target too: consumption scales
        // by 1/faux_clock, so this is what keeps the queue's depth *in
        // fills* — the margin that actually rides out jitter — constant
        // at every speed (~4.7 fills at 512-frame fills), instead of
        // fast-forward thinning it toward a short fill per trough.
        let target = source_rate * AUDIO_TARGET_QUEUED_SECS / faux_clock;
        // A speed-up moves the target out from under the queue in one
        // step, and the servo's authority would take seconds to deepen
        // it — riding thin (short fills, dropped audio) the whole way.
        // Re-prime instead: one clean gap on the edge the user just
        // caused, then a queue at full depth. A slow-down needs nothing;
        // the discard cap sheds the surplus in one skip.
        if target > self.last_target * 1.5 {
            self.priming = true;
        }
        self.last_target = target;

        let mut queued = pull.source_available() as f64;

        if queued > target * AUDIO_DISCARD_FACTOR {
            // Producer-burst backlog (seek chase, perspective swap,
            // device-stall catch-up): skip the oldest samples in one go
            // rather than carrying seconds of extra latency.
            pull.discard_source((queued - target) as usize);
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
        pull.process(
            source_rate * (1.0 + trim),
            self.out_rate as f64 * faux_clock,
            frame_count,
        );

        let delivered = pull.available().min(frame_count);
        let linear_buf: &mut [i16] = bytemuck::cast_slice_mut(buf);
        pull.read(&mut linear_buf[..delivered * super::NUM_CHANNELS], delivered);

        // A fill with nothing at all to give AND nothing queued is the
        // stall signature — and the moment an artifact was unavoidable
        // anyway. Latch priming so recovery is one clean gap instead of
        // seconds of jitter-trough crackle at a near-empty queue.
        // (Nothing at all, not merely short: re-priming answers with a
        // whole target's worth of silence, which is a fine trade
        // against a stalled sim and a terrible one against a fill that
        // came up a few frames light. And only with the queue empty
        // too: a run of unreachable reaches under drive-thread lock
        // contention also delivers nothing, but its backlog is sitting
        // right there — the next successful reach refills in one go,
        // so re-priming would stretch a one-fill blip into a whole
        // target of silence.)
        if delivered == 0 && queued < target {
            self.priming = true;
        }
        // Sustained partial starvation: a sim that can't hold its
        // published pace (DS-class load) produces slightly less than
        // every fill consumes, so fills come up a few frames short —
        // never empty, so the stall latch above never fires — and the
        // host splices a sliver of silence into every one, indefinitely.
        // A few of those in a row with the queue genuinely low is that
        // signature; concede and re-prime, trading continuous per-fill
        // crackle for one clean gap per drain cycle. The queue guard is
        // what keeps drive-thread lock contention out of this: a run of
        // unreachable reaches also delivers short, but its backlog is
        // sitting right there and the level (served stale across
        // contention) still reads it.
        if delivered < frame_count && queued < target {
            self.starved_fills += 1;
            if self.starved_fills >= STARVED_FILLS_TO_REPRIME {
                self.priming = true;
            }
        } else {
            self.starved_fills = 0;
        }
        if self.priming {
            self.starved_fills = 0;
        }
        delivered
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::super::{Drain, Source, NUM_CHANNELS};
    use super::*;

    const RATE: f64 = 32768.0;
    const OUT_RATE: u32 = 48000;
    const FILL: usize = 512;

    /// A console ring the test produces into by hand.
    struct Ring(Arc<Mutex<f64>>);

    impl Drain for Ring {
        fn sample_rate(&self) -> f64 {
            RATE
        }

        fn drain(&mut self, out: &mut [i16]) -> Option<usize> {
            let mut level = self.0.lock().unwrap();
            let total = *level as usize;
            let written = total.min(out.len() / 2);
            *level -= written as f64;
            out[..written * 2].fill(1);
            Some(total)
        }
    }

    /// One run's observations: the queue level the servo saw on each
    /// fill (in source frames — the audio latency), which fills the
    /// queues could not cover, and which of those were partial — some
    /// frames delivered, the rest spliced silence, the ugliest artifact
    /// a fill can produce.
    struct Run {
        queued: Vec<f64>,
        short: Vec<usize>,
        partial: Vec<usize>,
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
        let mut stream = Stream::new(
            pull,
            NATIVE_FPS as f32,
            Stream::fps_from_bits(published.clone()),
            OUT_RATE,
        );

        let secs_per_fill = FILL as f64 / OUT_RATE as f64;
        let mut buf = vec![[0i16; NUM_CHANNELS]; FILL];
        let mut run = Run {
            queued: Vec::new(),
            short: Vec::new(),
            partial: Vec::new(),
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
            run.queued.push(stream.pull.lock().unwrap().source_available() as f64);
            let delivered = stream.fill(&mut buf);
            if delivered < FILL {
                run.short.push(i);
            }
            if 0 < delivered && delivered < FILL {
                run.partial.push(i);
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
        assert!(
            run.short.is_empty(),
            "fills went short at steady state: {:?}",
            run.short
        );
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
        assert!(run.short.is_empty(), "a rollback burst starved fills: {:?}", run.short);
        // Absorbed, not shed: the queue swings with the burst but never
        // reaches the cap, which would be an audible skip.
        assert_settled(&run, 0.4, AUDIO_DISCARD_FACTOR);
    }

    /// Fast-forward is a step change in how fast the console produces
    /// AND how fast the device consumes source. The faux clock carries
    /// both through: the queue deepens to the scaled target (one
    /// re-prime gap on the edge — the moment the user pressed the
    /// button), then rides there without piling into the discard cap.
    #[test]
    fn fast_forward_lands_without_a_flood() {
        // 4x from fill 1000 on, production following.
        let speed = |i: usize| if i < 1000 { NATIVE_FPS } else { NATIVE_FPS * 4.0 };
        let run = run(3000, speed, promptly);
        // The target scales with speed, holding the queue's depth in
        // fills constant.
        let target = RATE * AUDIO_TARGET_QUEUED_SECS * 4.0;

        // The only silence is the re-prime right at the edge, while the
        // deeper queue builds.
        assert!(
            run.short.iter().all(|&i| (1000..1040).contains(&i)),
            "fills went short outside the flip's re-prime: {:?}",
            run.short
        );
        let settled = &run.queued[1100..];
        let low = settled.iter().copied().fold(f64::MAX, f64::min);
        let peak = settled.iter().copied().fold(0.0, f64::max);
        assert!(
            low > target * 0.6 && peak < target * 1.5,
            "queue ranged {low:.0}..{peak:.0} source frames at 4x, target {target:.0}"
        );
        assert!(
            peak < target * AUDIO_DISCARD_FACTOR,
            "queue piled up to {peak:.0} source frames, past a discard cap of {:.0}",
            target * AUDIO_DISCARD_FACTOR
        );
    }

    /// A sim that can't hold its published pace — DS-class load — makes
    /// every fill a little, never nothing, so the total-stall latch
    /// can't see it: before the starvation latch, this scenario spliced
    /// a sliver of silence into every single settled fill (5799 of
    /// 5800), indefinitely. The deficit can't be conjured away; what
    /// the latch buys is where it lands — whole clean gaps (re-prime
    /// silences) between runs of full fills, with partial fills bounded
    /// by the trigger window.
    #[test]
    fn sustained_overload_concedes_clean_gaps_not_per_fill_crackle() {
        let run = run(6000, native, |_, produced, ring| {
            // 8% short of the published pace, forever.
            *ring.lock().unwrap() += produced * 0.92;
        });
        let settled: Vec<usize> = run.short.iter().copied().filter(|&i| i >= 200).collect();
        assert!(!settled.is_empty(), "an 8% deficit must still cost something");
        // Most fills play complete audio…
        assert!(
            settled.len() * 4 < 5800,
            "starvation ate {} of 5800 settled fills",
            settled.len()
        );
        // …and the incomplete ones are re-prime silence, not splices:
        // partial fills appear only as the latch's trigger runs.
        let mut longest = 0usize;
        let mut current = 0usize;
        let mut prev = None::<usize>;
        for &i in run.partial.iter().filter(|&&i| i >= 200) {
            current = if prev == Some(i - 1) { current + 1 } else { 1 };
            longest = longest.max(current);
            prev = Some(i);
        }
        assert!(
            longest <= STARVED_FILLS_TO_REPRIME as usize,
            "a run of {longest} partial fills — the starvation latch should have conceded"
        );
    }

    /// A console reachable only at the drive loop's own moment — the
    /// saturated-loop case, where the fill's try-lock never lands and
    /// everything the stream plays has to have crossed through the
    /// intake's pump. `armed` is the between-ticks window: the pump
    /// consumes it; every other reach finds the console held.
    struct PumpOnly {
        ring: Ring,
        armed: Arc<std::sync::atomic::AtomicBool>,
    }

    impl Drain for PumpOnly {
        fn sample_rate(&self) -> f64 {
            self.ring.sample_rate()
        }

        fn drain(&mut self, out: &mut [i16]) -> Option<usize> {
            if !self.armed.swap(false, Ordering::Relaxed) {
                return None;
            }
            self.ring.drain(out)
        }
    }

    /// The saturated-drive-loop case (a DS pair at 4x holds its lock
    /// ~100% of wall time): the fill's own reaches never land, so
    /// without the intake this played wall-to-wall silence. Pumped once
    /// per "tick" at the one reachable moment, every fill plays full.
    #[test]
    fn a_fill_lives_off_the_intake_when_its_own_reach_never_lands() {
        let ring = Arc::new(Mutex::new(RATE * AUDIO_TARGET_QUEUED_SECS));
        let armed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let pull = Box::new(PumpOnly {
            ring: Ring(ring.clone()),
            armed: armed.clone(),
        });
        let published = Arc::new(std::sync::atomic::AtomicU32::new((NATIVE_FPS as f32).to_bits()));
        let mut stream = Stream::new(
            pull,
            NATIVE_FPS as f32,
            Stream::fps_from_bits(published.clone()),
            OUT_RATE,
        );
        let intake = stream.intake();

        let secs_per_fill = FILL as f64 / OUT_RATE as f64;
        let mut buf = vec![[0i16; NUM_CHANNELS]; FILL];
        let mut short = Vec::new();
        for i in 0..600 {
            // The drive loop's tick: produce, then pump at the moment
            // the lock is free.
            *ring.lock().unwrap() += RATE * secs_per_fill;
            armed.store(true, Ordering::Relaxed);
            intake.pump();
            // The fill's own reach finds the console held again.
            if stream.fill(&mut buf) < FILL {
                short.push(i);
            }
        }
        assert!(
            short.iter().all(|&i| i < 100),
            "fills starved despite a pumped intake: {:?}",
            &short.iter().filter(|&&i| i >= 100).collect::<Vec<_>>()
        );
    }

    /// The regression that motivated the playback-seconds target: sized
    /// in source seconds, the queue at 4x held barely more than one
    /// fill's consumption, so production arriving even a couple of
    /// fills late — a drive thread losing the race with the callback,
    /// an unreachable console under lock contention, both far likelier
    /// at 4x — went short on every trough, and fast-forward audibly
    /// dropped audio. With the depth in fills held constant, delivery
    /// lag rides out of the queue.
    #[test]
    fn fast_forward_survives_lumpy_delivery() {
        // Steady 4x; three fills' production lands at once, at the END
        // of each three-fill cycle — two dry fills lead every lump.
        let run = run(
            4000,
            |_| NATIVE_FPS * 4.0,
            |i, produced, ring| {
                if i % 3 == 2 {
                    *ring.lock().unwrap() += 3.0 * produced;
                }
            },
        );
        // Construction primes across the first lumps; after that, no
        // fill goes short.
        assert!(
            run.short.iter().all(|&i| i < 100),
            "lumpy 4x delivery starved fills: {:?}",
            &run.short.iter().filter(|&&i| i >= 100).collect::<Vec<_>>()
        );
    }
}
