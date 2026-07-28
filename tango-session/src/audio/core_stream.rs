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
//!   fps target (read through a closure, since the host drive loop
//!   owns pacing): a throttled sim stretches playback by the same
//!   ratio and a fast-forwarded one compresses into it, instead of
//!   starving or flooding the device. Within a few percent of 1× —
//!   PvP's clock-sync shaves, the GBA's 59.7275 Hz against a 60 fps
//!   target — the scale folds into the resample ratio, where it reads
//!   as inaudible pitch. Past the hysteresis band (real fast-forward
//!   or slow-mo) that fold would read as chipmunk or drone, so the
//!   resampler stays pitch-true and a WSOLA time stretcher
//!   (`stretch.rs`) absorbs the tempo difference instead — at the cost
//!   of a bounded prefetch's worth of extra latency while engaged, and
//!   possibly one re-prime gap at the transition (the stretcher's
//!   window fills out of the queue).
//! - The occupancy servo. A bare rate match leaves the queue level
//!   only neutrally stable — production minus consumption integrates
//!   into the level, so any perturbation (a stall draining the queue,
//!   a drive-loop cadence resync, OS-clock-vs-DAC-crystal drift of
//!   tens to hundreds of ppm whose sign is random per machine) would
//!   permanently re-base it, ratcheting toward chronic underrun
//!   crackle or latency creep. Trimming the claimed source rate
//!   against the level error turns the target level into an attractor.
//! - The discard cap sheds, in one skip, a backlog only a producer
//!   burst can create — a replay seek chase, a perspective swap onto
//!   an undrained core's full ring, a device stall's catch-up.
//!   (Rollbacks can't get there: re-simulation replaces its revoked
//!   audio exactly.)
//! - Re-priming. A fully drained queue means the sim stalled; refilled
//!   at the servo's authority alone it would ride near-empty for ~10
//!   seconds, where every jitter trough is an audible underrun. After
//!   a drain the stream serves silence until the queue reaches target
//!   once — one clean ~50 ms gap instead of seconds of crackle.
//!
//! The core access locks the same mutex the host's per-tick step
//! takes, so readout interleaves between ticks. A stalled sim
//! (reconnect pause, replay pause, a parked drive loop) still drains
//! the queue and goes silent — there's genuinely nothing to play.

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
/// phase swing), and rollbacks can't get here (re-simulation replaces
/// its revoked audio exactly) — only producer bursts do: a replay seek
/// chase, a perspective swap onto an undrained core's full ring, a
/// device stall's catch-up. One skip beats seconds of extra latency
/// (the servo alone would need ~10 s per 50 ms of backlog).
const AUDIO_DISCARD_FACTOR: f64 = 3.0;

/// Speed-factor deviation from 1× past which `fill` switches from the
/// rate-domain fold to the time stretcher, and the deviation below
/// which it switches back. The gap between them is hysteresis: PvP's
/// clock-sync shaves hover in the low percents and must not flap the
/// stretcher in and out (every transition is a reset — an audible
/// seam), while a real fast-forward or slow-mo jumps well past the
/// ceiling in one hop. Below the floor the fold costs at most ~34
/// cents of pitch — the cheaper artifact, with none of the stretcher's
/// window latency.
const STRETCH_ENGAGE_DEV: f64 = 0.05;
const STRETCH_DISENGAGE_DEV: f64 = 0.02;

pub struct CoreStream {
    /// The console's audio, already resampled to the device rate by
    /// whichever backend produced it. This stream never learns which
    /// emulator that was.
    pull: Box<dyn tango_match::AudioPull>,
    /// The host drive loop's current pacing target, f32. Zero or less is
    /// treated as unthrottled (60 fps).
    fps_target: Box<dyn Fn() -> f32 + Send>,
    out_rate: u32,
    /// Sink for samples dropped at the discard cap.
    discard: Vec<i16>,
    /// Tempo absorber for real speed multiples (see the module doc's
    /// faux-clock bullet); `engaged` is the hysteresis state and
    /// `speed` the last fill's measured factor (`1 / faux_clock`) —
    /// mode selection runs on it because the current factor isn't
    /// known until the core lock is already held.
    stretcher: super::stretch::TimeStretcher,
    engaged: bool,
    speed: f64,
    /// Staging for resampled frames on their way into the stretcher.
    scratch: Vec<i16>,
    /// Serve silence until the queue reaches target once. Latched at
    /// construction (the session starts with an empty queue) and again
    /// whenever the queue fully drains — the signature of a stalled sim.
    /// Without it, the post-stall rebuild happens at the servo's ~5 ms of
    /// queue per second and the stream rides near-empty for ~10 seconds,
    /// crackling on every jitter trough; one clean ~50 ms gap beats that.
    /// (The wall-clock-era reservoir learned this; the mechanism was lost
    /// in the revert.)
    priming: bool,
}

impl CoreStream {
    pub fn new(
        pull: impl tango_match::AudioPull + 'static,
        fps_target: impl Fn() -> f32 + Send + 'static,
        out_rate: u32,
    ) -> Self {
        Self {
            pull: Box::new(pull),
            fps_target: Box::new(fps_target),
            out_rate: if out_rate == 0 { 48000 } else { out_rate },
            discard: Vec::new(),
            stretcher: super::stretch::TimeStretcher::new(if out_rate == 0 { 48000 } else { out_rate }),
            engaged: false,
            speed: 1.0,
            scratch: Vec::new(),
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

        // Stretcher hysteresis, on the previous fill's measured speed
        // (one fill of lag on a transition is ~10 ms — inaudible next
        // to the transition itself).
        let deviation = (self.speed - 1.0).abs();
        let engaged = if self.engaged {
            deviation >= STRETCH_DISENGAGE_DEV
        } else {
            deviation > STRETCH_ENGAGE_DEV
        };
        if engaged != self.engaged {
            // Both directions restart clean: leftover resampled frames
            // were produced under the other mode's rate claim.
            self.engaged = engaged;
            self.stretcher.reset();
            // Leftover resampled frames were produced under the other
            // mode's rate claim.
            let stale = self.pull.available();
            let mut sink = vec![0i16; stale * super::NUM_CHANNELS];
            self.pull.read(&mut sink, stale);
        }

        // How many resampled device-rate frames this fill wants out of
        // the ring: the fill itself in the rate-domain mode, the
        // stretcher's appetite (≈ frame_count × speed, plus its window
        // while it fills) when engaged.
        let want = if engaged {
            self.stretcher.input_deficit(frame_count, self.speed)
        } else {
            frame_count
        };

        // The console's production rate can change at runtime (BN4+
        // flip from 32768 to 65536 Hz after boot), so it is re-read
        // every fill.
        let rate = self.pull.sample_rate();
        // The faux clock: production scales with the sim's pace, so a
        // throttled sim stretches playback by the same ratio instead of
        // starving it (and a fast-forwarded one compresses).
        let faux_clock = self.pull.framerate_ratio(fps_target as f64);
        let speed = 1.0 / faux_clock;

        let target = rate * AUDIO_TARGET_QUEUED_SECS;
        let queued = self.pull.source_available() as f64;
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
                self.speed = speed;
                // Silence until the queue rebuilds.
                return 0;
            }
            self.priming = false;
        }

        // Servo: nudge the claimed source rate so the queue level
        // converges on the target. Claiming the source faster than it
        // is makes each output frame consume more of it (drains);
        // slower, less (refills).
        let trim = AUDIO_MAX_TRIM * ((queued - target) / target).clamp(-1.0, 1.0);
        // Engaged, the destination claim is honest — pitch-true — and
        // the stretcher downstream absorbs the tempo difference;
        // otherwise the faux clock folds in here.
        let dest_rate = if engaged {
            self.out_rate as f64
        } else {
            self.out_rate as f64 * faux_clock
        };
        self.pull.process(rate * (1.0 + trim), dest_rate);

        self.speed = speed;

        let delivered = if engaged {
            let got = self.pull.available().min(want);
            self.scratch.resize(got * super::NUM_CHANNELS, 0);
            self.pull.read(&mut self.scratch, got);
            self.stretcher.push(&self.scratch);
            self.stretcher.pop(buf, self.speed)
        } else {
            let available = self.pull.available().min(frame_count);
            let linear_buf: &mut [i16] = bytemuck::cast_slice_mut(buf);
            self.pull
                .read(&mut linear_buf[..available * super::NUM_CHANNELS], available);
            available
        };

        // A fill the queue couldn't cover in full is the stall
        // signature — and the moment an artifact was unavoidable
        // anyway. Latch priming so recovery is one clean gap instead
        // of seconds of jitter-trough crackle at a near-empty queue.
        if delivered < frame_count {
            self.priming = true;
        }
        delivered
    }
}
