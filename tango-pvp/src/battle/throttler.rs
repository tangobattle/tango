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
//! Layered on top is an *emergency* stall ([`emergency_slowdown`]). The
//! steady-state controller above is a gentle smoother capped at
//! [`MAX_SLOWDOWN`], and by itself it cannot stop a runaway: a peer slower than
//! `EXPECTED_FPS - MAX_SLOWDOWN`, or one that briefly goes silent, drives the
//! unmatched-local lead toward the engine's overflow bail regardless. Once the
//! lead crosses the high-water mark (supplied to [`Throttler::new`]) a
//! high-gain brake on the *raw* lead takes over and nearly freezes the local
//! frontier, holding the lead below the cap so a slow peer is matched instead
//! of killed. (A *fully* silent peer can't be matched at any fps; the round's
//! stall watchdog tears that one down on a timeout.)

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

/// Floor fps the emergency stall ([`emergency_slowdown`]) brakes down to. Not
/// zero — mGBA's frame pacing reads `fpsTarget == 0` as "unbounded", not
/// "stopped" — but low enough to nearly freeze the local frontier while the
/// peer catches up.
const MIN_STALL_FPS: f32 = 2.0;
/// Emergency-brake gain: fps shaved per frame of lead beyond the high-water
/// mark. Steep, so even a peer confirming at only a few fps reaches its holding
/// equilibrium just past the mark, well below the overflow cap.
const STALL_GAIN: f32 = 3.0;

/// Emergency time-sync stall, layered over the steady-state controller in
/// [`Throttler::step`].
///
/// `lead` is the raw unmatched-local count (the same quantity the engine's
/// overflow bail guards) and `high_water` is the lead at which the brake
/// engages (supplied to [`Throttler::new`] by the caller, which owns the
/// overflow cap this is a fraction of). Below `high_water` this is silent and
/// the smoothed controller alone shapes the output. Past it, a high-gain
/// proportional brake on the *raw* lead — no EMA, it must react the same frame —
/// ramps the slowdown up to a near-full stall (`EXPECTED_FPS - MIN_STALL_FPS`).
/// It settles at an equilibrium just above the mark where the local frontier
/// advances at the peer's actual confirm rate, holding the lead below the cap
/// for an arbitrarily slow peer. A *fully* silent peer can't be held — the
/// frontier still creeps at the floor fps while confirmation stays put — so the
/// round's stall watchdog tears that case down on a timeout.
fn emergency_slowdown(lead: i32, high_water: i32) -> f32 {
    let excess = (lead - high_water).max(0) as f32;
    (STALL_GAIN * excess).clamp(0.0, super::EXPECTED_FPS - MIN_STALL_FPS)
}

/// Per-round time-sync throttler. [`Round`](super::Round) owns one and feeds it
/// the engine's raw skew and speculation balance each frame.
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
    /// Lead at which the emergency stall ([`emergency_slowdown`]) engages,
    /// supplied at construction. The throttler doesn't own the overflow cap
    /// this is a fraction of — the caller ([`Round`](super::Round)) does — so it
    /// takes the threshold as a plain value rather than reaching for the
    /// constant.
    high_water: i32,
}

impl Throttler {
    /// `high_water` is the unmatched-local lead at which the emergency brake
    /// engages — a fraction of the caller's overflow cap (see
    /// [`emergency_slowdown`]).
    pub(crate) fn new(high_water: i32) -> Self {
        Self {
            smoothed: 0.0,
            smoothed_balance: 0.0,
            high_water,
        }
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
    /// running ahead costs no presentation quality — so the steady-state result
    /// is 0 no matter how large the skew. Past the boundary the permitted
    /// slowdown ramps up at ENGAGEMENT_SLOPE fps per tick of smoothed depth
    /// until the smoothed skew takes over; the steady-state result is always in
    /// `[0, MAX_SLOWDOWN]`.
    ///
    /// `lead` is the raw unmatched-local count, fed to the emergency stall
    /// ([`emergency_slowdown`]) layered on top: past the high-water mark
    /// supplied to [`new`](Self::new) it can drive the result well above
    /// `MAX_SLOWDOWN`, up to `EXPECTED_FPS - MIN_STALL_FPS`, to brake a runaway
    /// the smoother can't. The final result is the larger of the two (0 = run at
    /// full speed).
    pub(crate) fn step(&mut self, skew: i32, speculation_balance: i32, lead: i32) -> f32 {
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
        let steady = (SKEW_TO_SLOWDOWN * self.smoothed)
            .min(ENGAGEMENT_SLOPE * self.smoothed_balance.max(0.0))
            .clamp(0.0, MAX_SLOWDOWN);
        steady.max(emergency_slowdown(lead, self.high_water))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A speculation balance deep past the boundary, so the engagement gate
    /// never binds and the smoothed skew alone shapes the output.
    const OPEN: i32 = 100;

    /// Representative high-water mark for the constructed throttler — a stand-in
    /// for [`STALL_HIGH_WATER`](super::super::round::STALL_HIGH_WATER) (70% of the
    /// engine's overflow cap), kept as a local literal so these tests don't
    /// depend on the round's policy constant.
    const HW: i32 = 84;

    /// A lead well below the emergency high-water mark, so [`emergency_slowdown`]
    /// stays silent and these tests exercise only the steady-state controller.
    const NEAR: i32 = 0;

    fn run(t: &mut Throttler, skew: i32, balance: i32, frames: u32) {
        for _ in 0..frames {
            t.step(skew, balance, NEAR);
        }
    }

    /// Frames of sustained `skew` until the emitted slowdown reaches `target`.
    fn frames_until(t: &mut Throttler, skew: i32, balance: i32, target: f32) -> u32 {
        for n in 1..=10_000 {
            if t.step(skew, balance, NEAR) >= target {
                return n;
            }
        }
        panic!("slowdown never reached {target}");
    }

    /// Sustained lead engages the brake on the designed slow rise: skew held
    /// at 20 crosses a 10 fps slowdown in ≈208 frames (~3.5 s at 60 Hz).
    #[test]
    fn onset_is_gentle() {
        let n = frames_until(&mut Throttler::new(HW), 20, OPEN, 10.0);
        assert!((190..=225).contains(&n), "engaged after {n} frames");
    }

    /// Once skew settles back to zero, the fast fall lets the brake go in
    /// under a second.
    #[test]
    fn release_is_fast() {
        let mut t = Throttler::new(HW);
        let mut n = 0;
        while t.step(120, OPEN, NEAR) < MAX_SLOWDOWN {
            n += 1;
            assert!(n < 1_000, "never hit the cap");
        }
        let mut n = 0;
        while t.step(0, OPEN, NEAR) > 5.0 {
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
        let fresh = frames_until(&mut Throttler::new(HW), 20, OPEN, 10.0);
        let mut t = Throttler::new(HW);
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
        let mut t = Throttler::new(HW);
        run(&mut t, 20, -2, 5_000);
        assert_eq!(t.step(20, -2, NEAR), 0.0);
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
        let mut t = Throttler::new(HW);
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
        let mut t = Throttler::new(HW);
        let flap = |i: u32| if i % 2 == 0 { 2 } else { -2 };
        for i in 0..5_000 {
            t.step(30, flap(i), NEAR);
        }
        let outs: Vec<f32> = (0..200).map(|i| t.step(30, flap(i), NEAR)).collect();
        let hi = outs.iter().copied().fold(f32::MIN, f32::max);
        let lo = outs.iter().copied().fold(f32::MAX, f32::min);
        assert!(hi <= 1.0, "throttled {hi} fps with the mean lead at the boundary");
        assert!(hi - lo <= 0.5, "target fluttered by {} fps", hi - lo);
    }

    /// The *steady-state* slowdown is capped at MAX_SLOWDOWN — but only with the
    /// lead below the high-water mark, where the emergency brake is silent.
    #[test]
    fn slowdown_is_capped() {
        let mut t = Throttler::new(HW);
        let peak = (0..5_000).map(|_| t.step(1_000, OPEN, NEAR)).fold(0.0f32, f32::max);
        assert_eq!(peak, MAX_SLOWDOWN);
    }

    /// Below the high-water mark the emergency brake contributes nothing; the
    /// steady-state cap still holds even at an enormous skew.
    #[test]
    fn emergency_silent_below_high_water() {
        assert_eq!(emergency_slowdown(0, HW), 0.0);
        assert_eq!(emergency_slowdown(HW, HW), 0.0);
        let mut t = Throttler::new(HW);
        let peak = (0..5_000).map(|_| t.step(1_000, OPEN, HW)).fold(0.0f32, f32::max);
        assert_eq!(peak, MAX_SLOWDOWN, "emergency leaked below the high-water mark");
    }

    /// Past the high-water mark the brake ramps up and saturates at the floor
    /// (`EXPECTED_FPS - MIN_STALL_FPS`) within a couple dozen frames of lead —
    /// `STALL_GAIN` fps each — so the lead is arrested with headroom to spare
    /// below the caller's overflow cap.
    #[test]
    fn emergency_brakes_to_floor_past_high_water() {
        let floor = super::super::EXPECTED_FPS - MIN_STALL_FPS;
        assert!(emergency_slowdown(HW + 1, HW) > 0.0, "brake stayed off just past the mark");
        // Ramps linearly at STALL_GAIN until clamped.
        assert_eq!(emergency_slowdown(HW + 5, HW), STALL_GAIN * 5.0);
        // Saturated within ceil(floor / STALL_GAIN) frames of excess, and stays
        // there however far past the mark the lead runs.
        let to_floor = (floor / STALL_GAIN).ceil() as i32;
        assert_eq!(emergency_slowdown(HW + to_floor, HW), floor);
        assert_eq!(emergency_slowdown(HW + 1_000, HW), floor);
    }

    /// The emergency brake overrides a quiescent steady controller: even at zero
    /// skew, a lead well past the high-water mark stalls the sim to the floor.
    #[test]
    fn emergency_overrides_steady_state() {
        let mut t = Throttler::new(HW);
        let out = t.step(0, OPEN, HW + 100);
        assert_eq!(out, super::super::EXPECTED_FPS - MIN_STALL_FPS);
    }
}
