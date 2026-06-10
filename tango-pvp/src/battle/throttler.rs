//! Time-sync throttler. Converts the engine's raw per-frame skew
//! (`local_advantage - remote_advantage`) into a slowdown in fps below the base
//! rate; [`Round`](super::Round) turns that into an absolute fps target for the
//! live core. Only the leading peer slows down — the trailing peer runs at full
//! rate and lets the leader ease back toward it. Throttling also only engages
//! once the presented frame actually speculates past the present delay: while
//! the lead still fits inside it, running ahead costs no presentation quality,
//! so no fps is shaved. (It still costs CPU — the frontier speculates and
//! re-sims mispredictions regardless of what's presented — but that's the
//! frame path's budget, not the player's.)
//!
//! A second, much slower loop rate-matches the hosts. A persistent pacing
//! mismatch between the two (timer- vs vsync- vs audio-clock-paced) would
//! otherwise march the lead into the speculation boundary over and over,
//! each pass tapping a brake the player can feel. The estimator reads the
//! drift off the per-frame skew deltas while the fast loop is quiet and
//! folds it into a small persistent trim, applied *outside* the engagement
//! gate — rate matching is exactly the correction that must act while
//! presentation is still free, before the lead reaches the boundary at all.
//! The trim slowly leaks (see [`TRIM_LEAK`]), so a trim whose cause has
//! gone away dies out instead of lingering for the rest of the match.

/// EMA weight applied while skew is growing. τ ≈ 5 s rise (at 60 Hz).
const ALPHA_SLOWDOWN: f32 = 1.0 / 300.0;
/// EMA weight applied while skew is shrinking. τ ≈ 0.5 s fall (at 60 Hz).
const ALPHA_SPEEDUP: f32 = 1.0 / 30.0;
/// EMA weight for the speculation balance. τ ≈ 0.5 s (at 60 Hz): the raw
/// balance moves in bursts as remote inputs arrive, and smoothing it keeps
/// that flutter out of the emitted fps target.
const ALPHA_BALANCE: f32 = 1.0 / 30.0;
/// Gain from smoothed skew to emitted slowdown, in fps shaved per tick of
/// skew. At 1.0 the throttler is a proportional controller whose correction
/// rate equals its error: a smoothed skew of S sheds lead at ~S ticks/s,
/// converging exponentially with a ~1 s time constant once engaged.
const SKEW_TO_SLOWDOWN: f32 = 1.0;
/// Engagement ramp at the speculation boundary: fps of slowdown permitted per
/// tick of smoothed speculative depth. Deliberately steep — the ramp decides
/// *when* throttling engages (at the boundary, continuously), not how hard;
/// magnitude is still the smoothed skew's job, which takes over within the
/// first few ticks of depth.
const ENGAGEMENT_SLOPE: f32 = 10.0;
/// Slowdown ceiling, in fps below the base rate.
const MAX_SLOWDOWN: f32 = 30.0;
/// Floor on the smoothed speculation balance. Deep headroom (lead pinned well
/// inside the present delay) would otherwise wind the balance EMA down toward
/// -present_delay, and the climb back through zero would defer engagement past
/// the boundary by however long the slider made it — the skew windup all over
/// again, scaled by a user setting. The ramp saturates the slowdown ceiling at
/// MAX_SLOWDOWN/ENGAGEMENT_SLOPE ticks of depth, so flooring at its mirror
/// image keeps the gate's swing symmetric around the boundary and the
/// re-engagement latency constant, while leaving enough negative range to
/// absorb bursty-arrival flutter.
const BALANCE_FLOOR: f32 = -(MAX_SLOWDOWN / ENGAGEMENT_SLOPE);
/// Fast-loop output below which the trim estimator counts the loop as quiet
/// and accepts skew samples. An intentional fast slowdown moves skew exactly
/// the way clock drift does, so sampling while it acts would fold our own
/// correction into the rate estimate; below this threshold the contamination
/// is bounded and biased toward under-trimming (the safe direction).
const TRIM_QUIET: f32 = 0.25;
/// Largest per-frame skew delta the trim estimator accepts, in ticks. Real
/// rate mismatch is far below a tick per frame, so it reaches the integer
/// skew as occasional ±1 steps; bigger jumps are network bursts and stalls —
/// symmetric flutter at best, a stall's one-sided swing at worst — and are
/// excluded rather than averaged.
const TRIM_DELTA_GUARD: i32 = 3;
/// Accepted samples per trim-estimation window. The window's deltas telescope
/// to a two-point slope of the skew level, so the noise is the level's
/// endpoint flutter (a tick or two) against a signal that grows with window
/// length: over 900 clean steps (~15 s), 0.3 fps of drift reads ≈ 9 ticks.
const TRIM_WINDOW: u32 = 900;
/// Fraction of each window's measured residual rate folded into the trim.
/// Geometric convergence — the residual halves each window, ~45 s from a cold
/// start — with enough damping that one noisy window moves the trim ≲0.05 fps.
const TRIM_CORRECTION: f32 = 0.5;
/// Trim ceiling, in fps. Sized to the plausible pacing gaps (a vsync-paced
/// 60.00 Hz host against an audio-paced 59.73 Hz one is 0.27 fps) while
/// bounding any estimator failure to an imperceptible rate change.
const TRIM_MAX: f32 = 0.5;
/// Per-window decay of the trim. Without it the two peers' trims have a
/// conserved sum: one peer's extra slowness is the other's measured drift,
/// so coupled estimators move by equal and opposite amounts each window, and
/// an overshoot — a real gap that saturates the trim and then vanishes, say
/// a vsync toggle — redistributes between the peers instead of decaying,
/// parking both somewhere slow for the rest of the match. The leak drains
/// that sum; real drift re-earns its trim against it, at the cost of a
/// steady-state under-trim of TRIM_LEAK/TRIM_CORRECTION (0.02 fps — a
/// boundary creep of ~2 ticks/min left for the fast loop to absorb).
const TRIM_LEAK: f32 = 0.01;
/// Nominal step rate for converting a per-step skew slope to fps. Only a gain
/// factor inside a damped loop, so the nominal value is plenty.
const STEPS_PER_SECOND: f32 = 60.0;

/// Time-sync throttler. The [`Match`](super::Match) owns one for its whole
/// lifetime and each [`Round`](super::Round) drives it: per-round state is
/// cleared at round start ([`reset_transients`](Throttler::reset_transients)),
/// then the engine's raw skew and speculation balance are fed to
/// [`step`](Throttler::step) each frame. The learned rate trim is a property
/// of the host pairing, not of any one round, and carries across rounds.
pub(crate) struct Throttler {
    /// Asymmetric-EMA-smoothed skew, carried across frames. Floored at zero:
    /// negative skew (the peer leading) would otherwise wind the average down —
    /// the fast fall digs the hole in under a second, and the slow rise then
    /// spends 10+ seconds climbing back out before a new lead of ours gets
    /// throttled at all. Above zero the value stays raw past MAX_SLOWDOWN; the
    /// fast fall drains any overshoot in a few hundred ms, so clamping there
    /// would buy nothing.
    smoothed: f32,
    /// EMA-smoothed speculation balance. The raw balance jumps frame to frame
    /// as remote inputs arrive in bursts; gating engagement on the smoothed
    /// value keeps that flutter out of the fps target. Floored at
    /// [`BALANCE_FLOOR`]: deep headroom would otherwise wind it down toward
    /// -present_delay, and the climb back through zero would hold the brake
    /// off well past the boundary crossing.
    smoothed_balance: f32,
    /// Learned clock-rate trim, in fps below the base rate. Applied outside
    /// the engagement gate every frame and kept across rounds; see the module
    /// docs.
    trim: f32,
    /// Raw skew last step, for the trim estimator's per-step delta.
    last_skew: i32,
    /// Sum of accepted skew deltas in the current estimation window — the
    /// telescoped change of the skew level over the window's clean samples.
    window_sum: i32,
    /// Count of accepted samples in the current estimation window.
    window_len: u32,
}

impl Throttler {
    pub(crate) fn new() -> Self {
        Self {
            smoothed: 0.0,
            smoothed_balance: 0.0,
            trim: 0.0,
            last_skew: 0,
            window_sum: 0,
            window_len: 0,
        }
    }

    /// Clear the per-round scratch — the fast loop's EMAs and the estimator's
    /// half-filled window — keeping the learned rate trim. Called at round
    /// start: skew and balance restart from fresh queues, but the hosts'
    /// pacing mismatch is the same as it was last round.
    pub(crate) fn reset_transients(&mut self) {
        self.smoothed = 0.0;
        self.smoothed_balance = 0.0;
        self.last_skew = 0;
        self.window_sum = 0;
        self.window_len = 0;
    }

    /// Compute the slowdown to apply this frame, in fps below the base rate.
    ///
    /// `skew` is the raw integer frame difference
    /// `local_advantage - remote_advantage`; its asymmetric EMA, scaled by
    /// [`SKEW_TO_SLOWDOWN`], sets the slowdown magnitude. `speculation_balance`
    /// is the engine's signed
    /// distance of the presented frame from the speculation boundary
    /// (`lead - present_delay`); its smoothed sign gates engagement. While the
    /// smoothed balance is negative the presented frame is fully confirmed —
    /// running ahead costs no presentation quality — so the result is 0 no
    /// matter how large the skew. Past the boundary the permitted slowdown
    /// ramps up at ENGAGEMENT_SLOPE fps per tick of smoothed depth until the
    /// smoothed skew takes over. On top of the gated fast slowdown rides the
    /// persistent rate trim (see the module docs), so the result is always in
    /// `[0, MAX_SLOWDOWN + TRIM_MAX]` (0 = run at full speed).
    pub(crate) fn step(&mut self, skew: i32, speculation_balance: i32) -> f32 {
        let delta = skew - self.last_skew;
        self.last_skew = skew;
        let skew = skew as f32;
        let alpha = if skew > self.smoothed {
            ALPHA_SLOWDOWN
        } else {
            ALPHA_SPEEDUP
        };
        self.smoothed = (alpha * skew + (1.0 - alpha) * self.smoothed).max(0.0);
        self.smoothed_balance = (ALPHA_BALANCE * speculation_balance as f32
            + (1.0 - ALPHA_BALANCE) * self.smoothed_balance)
            .max(BALANCE_FLOOR);
        let fast = (SKEW_TO_SLOWDOWN * self.smoothed)
            .min(ENGAGEMENT_SLOPE * self.smoothed_balance.max(0.0))
            .clamp(0.0, MAX_SLOWDOWN);

        // Rate-trim estimator: sample only while our own correction is quiet
        // and the delta is plausibly drift rather than a burst.
        if fast <= TRIM_QUIET && delta.abs() <= TRIM_DELTA_GUARD {
            self.window_sum += delta;
            self.window_len += 1;
            if self.window_len == TRIM_WINDOW {
                // A rate gap moves skew at twice its fps value (our advantage
                // rises as the peer's falls), hence the 2 in the conversion.
                let residual_fps =
                    self.window_sum as f32 * STEPS_PER_SECOND / (2.0 * TRIM_WINDOW as f32);
                self.trim =
                    (self.trim + TRIM_CORRECTION * residual_fps - TRIM_LEAK).clamp(0.0, TRIM_MAX);
                self.window_sum = 0;
                self.window_len = 0;
            }
        }

        fast + self.trim
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A speculation balance deep past the boundary, so the engagement gate
    /// never binds and the smoothed skew alone shapes the output.
    const OPEN: i32 = 100;

    fn run(t: &mut Throttler, skew: i32, balance: i32, frames: u32) {
        for _ in 0..frames {
            t.step(skew, balance);
        }
    }

    /// Frames of sustained `skew` until the emitted slowdown reaches `target`.
    fn frames_until(t: &mut Throttler, skew: i32, balance: i32, target: f32) -> u32 {
        for n in 1..=10_000 {
            if t.step(skew, balance) >= target {
                return n;
            }
        }
        panic!("slowdown never reached {target}");
    }

    /// Sustained lead engages the brake on the designed slow rise: skew held
    /// at 20 crosses a 10 fps slowdown in ≈208 frames (~3.5 s at 60 Hz).
    #[test]
    fn onset_is_gentle() {
        let n = frames_until(&mut Throttler::new(), 20, OPEN, 10.0);
        assert!((190..=225).contains(&n), "engaged after {n} frames");
    }

    /// Once skew settles back to zero, the fast fall lets the brake go in
    /// under a second.
    #[test]
    fn release_is_fast() {
        let mut t = Throttler::new();
        let mut n = 0;
        while t.step(120, OPEN) < MAX_SLOWDOWN {
            n += 1;
            assert!(n < 1_000, "never hit the cap");
        }
        let mut n = 0;
        while t.step(0, OPEN) > 5.0 {
            n += 1;
            assert!(n <= 60, "release took more than a second");
        }
    }

    /// Regression: a long stretch of the peer leading (deep negative skew —
    /// e.g. we just recovered from a stall) must not defer our own throttle
    /// onset once the lead flips back to us. Without the floor the EMA winds
    /// up around −120 and the slow rise takes ~790 frames (~13 s) to engage
    /// instead of ~208.
    #[test]
    fn peer_led_history_does_not_delay_onset() {
        let fresh = frames_until(&mut Throttler::new(), 20, OPEN, 10.0);
        let mut t = Throttler::new();
        run(&mut t, -120, -5, 300);
        let after_history = frames_until(&mut t, 20, OPEN, 10.0);
        assert_eq!(
            after_history, fresh,
            "negative windup deferred onset: {after_history} frames vs {fresh} fresh"
        );
    }

    /// A sustained lead that still fits inside the present delay (negative
    /// speculation balance) never shaves fps, no matter how large the smoothed
    /// skew has grown — and once the balance crosses the boundary, the brake
    /// comes on within a second.
    #[test]
    fn lead_inside_present_delay_is_free_until_the_boundary() {
        let mut t = Throttler::new();
        run(&mut t, 20, -2, 5_000);
        assert_eq!(t.step(20, -2), 0.0);
        let n = frames_until(&mut t, 20, 2, 10.0);
        assert!(n <= 60, "took {n} frames to engage past the boundary");
    }

    /// Regression: a long stretch of deep headroom (lead pinned far inside a
    /// large present delay) must not defer engagement once the lead finally
    /// crosses the boundary. Without the floor the balance EMA winds down
    /// toward -30 here and the climb back through zero holds the brake off
    /// for ~1.7 s instead of under a second.
    #[test]
    fn deep_headroom_does_not_delay_engagement() {
        let mut t = Throttler::new();
        // Saturate the skew EMA so the balance gate is what binds below.
        run(&mut t, 20, OPEN, 5_000);
        run(&mut t, 20, -30, 1_000);
        let n = frames_until(&mut t, 20, 2, 10.0);
        assert!(n <= 60, "took {n} frames to engage past the boundary");
    }

    /// A balance fluttering across the boundary with bursty remote input
    /// arrival (±2 ticks every frame, mean 0) neither throttles meaningfully
    /// nor flutters the fps target frame to frame.
    #[test]
    fn bursty_balance_does_not_flutter_the_target() {
        let mut t = Throttler::new();
        let flap = |i: u32| if i % 2 == 0 { 2 } else { -2 };
        for i in 0..5_000 {
            t.step(30, flap(i));
        }
        let outs: Vec<f32> = (0..200).map(|i| t.step(30, flap(i))).collect();
        let hi = outs.iter().copied().fold(f32::MIN, f32::max);
        let lo = outs.iter().copied().fold(f32::MAX, f32::min);
        assert!(hi <= 1.0, "throttled {hi} fps with the mean lead at the boundary");
        assert!(hi - lo <= 0.5, "target fluttered by {} fps", hi - lo);
    }

    #[test]
    fn slowdown_is_capped() {
        let mut t = Throttler::new();
        let peak = (0..5_000).map(|_| t.step(1_000, OPEN)).fold(0.0f32, f32::max);
        assert_eq!(peak, MAX_SLOWDOWN);
    }

    /// Closed-loop drift sim: the peer's clock runs `gap` fps slower than
    /// ours, so the raw skew integrates twice the *corrected* rate difference
    /// (our advantage rises as the peer's falls). The throttler's own output
    /// feeds back, closing the loop. Returns the final (fractional) skew.
    fn run_drift(t: &mut Throttler, gap: f32, balance: i32, steps: u32, mut skew: f32) -> f32 {
        for _ in 0..steps {
            let slowdown = t.step(skew.round() as i32, balance);
            skew += 2.0 * (gap - slowdown) / 60.0;
        }
        skew
    }

    /// A persistent 0.3 fps pacing gap with the lead inside the present delay
    /// (fast loop gated off) gets rate-matched: after two minutes the trim has
    /// converged on the gap and the skew has stopped marching.
    #[test]
    fn persistent_drift_gets_trimmed() {
        let mut t = Throttler::new();
        run_drift(&mut t, 0.3, -5, 7_200, 0.0);
        assert!((t.trim - 0.3).abs() < 0.07, "trim {} after 2 min of 0.3 fps drift", t.trim);
        let marched = run_drift(&mut t, 0.3, -5, 1_800, 0.0);
        assert!(marched.abs() <= 5.0, "skew still marched {marched} ticks in 30 s");
    }

    /// A pacing gap past the trim ceiling saturates the trim at TRIM_MAX
    /// instead of winding up beyond it.
    #[test]
    fn trim_is_clamped() {
        let mut t = Throttler::new();
        run_drift(&mut t, 5.0, -5, 3_600, 0.0);
        assert_eq!(t.trim, TRIM_MAX);
    }

    /// Mean-zero skew flutter with no real drift must not inflate the trim:
    /// each window telescopes to the level difference across it, so bounded
    /// flutter reads as (at most) endpoint jitter.
    #[test]
    fn flutter_does_not_inflate_trim() {
        let mut t = Throttler::new();
        let level = [0, 1, 2, 1, 0, -1, 0];
        for i in 0..20_000usize {
            t.step(level[i % level.len()], -5);
        }
        assert!(t.trim <= 0.06, "trim wound up to {} on mean-zero flutter", t.trim);
    }

    /// A stall's skew swing — big per-frame deltas out, a long hold, big
    /// deltas back — is excluded by the delta guard, not read as drift.
    #[test]
    fn stall_swings_are_not_read_as_drift() {
        let mut t = Throttler::new();
        run(&mut t, 0, -5, 1_000);
        for i in 1..=10 {
            t.step(9 * i, -5);
        }
        run(&mut t, 90, -5, 2_000);
        // Checked before the recovery too: the out-swing alone must not have
        // registered, not merely cancelled against the swing back.
        assert_eq!(t.trim, 0.0);
        for i in 1..=10 {
            t.step(90 - 9 * i, -5);
        }
        run(&mut t, 0, -5, 2_000);
        assert_eq!(t.trim, 0.0);
    }

    /// While the fast loop is braking it drags skew down just like drift
    /// would, so those frames must not feed the estimator: a converged trim
    /// survives a braking episode (a 60-tick lead drained at the boundary)
    /// essentially unchanged.
    #[test]
    fn braking_episode_does_not_unlearn_the_trim() {
        let mut t = Throttler::new();
        t.trim = 0.3;
        let end = run_drift(&mut t, 0.3, OPEN, 3_600, 60.0);
        assert!((t.trim - 0.3).abs() <= 0.05, "trim moved to {}", t.trim);
        assert!(end.abs() <= 5.0, "lead not drained: {end}");
    }

    /// The ceiling isn't sticky: when the gap vanishes, the trim unwinds —
    /// being over-trimmed makes us trail, trailing keeps the fast loop
    /// floored at zero, so the estimator keeps sampling and walks the trim
    /// back down (helped along by the leak).
    #[test]
    fn ceiling_unwinds_when_the_gap_vanishes() {
        let mut t = Throttler::new();
        run_drift(&mut t, 5.0, -5, 3_600, 0.0);
        assert_eq!(t.trim, TRIM_MAX);
        run_drift(&mut t, 0.0, -5, 7_200, 0.0);
        assert!(t.trim <= 0.05, "trim stuck at {}", t.trim);
    }

    /// Two coupled throttlers (the peer runs the same estimator), no real
    /// pacing gap, ours starting over-trimmed at the ceiling. Each side reads
    /// the other's trim as drift, and their window updates move the two trims
    /// by equal and opposite amounts — a conserved sum that, without the
    /// leak, parks the pair at 0.25/0.25 for the rest of the match. The leak
    /// drains it: both sides bleed back to zero.
    #[test]
    fn coupled_overshoot_drains_instead_of_redistributing() {
        let mut us = Throttler::new();
        us.trim = TRIM_MAX;
        let mut peer = Throttler::new();
        let mut skew = 0.0f32;
        for _ in 0..60_000 {
            let su = us.step(skew.round() as i32, -5);
            let sp = peer.step((-skew).round() as i32, -5);
            skew += 2.0 * (sp - su) / 60.0;
        }
        assert!(us.trim <= 0.05, "our trim stuck at {}", us.trim);
        assert!(peer.trim <= 0.05, "peer trim stuck at {}", peer.trim);
    }

    /// Round boundaries clear the fast loop's state but keep the learned
    /// trim — pacing mismatch belongs to the host pairing, not the round.
    #[test]
    fn reset_keeps_the_trim() {
        let mut t = Throttler::new();
        run_drift(&mut t, 0.3, -5, 7_200, 0.0);
        let trim = t.trim;
        assert!(trim > 0.2);
        run(&mut t, 120, OPEN, 1_000); // wind the fast loop up
        t.reset_transients();
        assert_eq!(t.trim, trim);
        // Fast loop quiet again from the first post-reset frame; only the
        // trim remains in the output.
        assert_eq!(t.step(0, -5), trim);
    }
}
