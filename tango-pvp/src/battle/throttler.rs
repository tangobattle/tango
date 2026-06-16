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
//! Layered on top is an *emergency* regime ([`emergency_slowdown`]). The
//! steady-state controller above is a gentle smoother capped at
//! [`MAX_SLOWDOWN`], and by itself it cannot stop a runaway: a peer slower than
//! `EXPECTED_FPS - MAX_SLOWDOWN` drives the unmatched-local lead toward the
//! engine's overflow bail regardless. Once the lead crosses the high-water mark
//! (supplied to [`Throttler::new`]) the cap lifts and a high-gain brake driven
//! by the *raw skew* takes over — strong enough to nearly freeze the local
//! frontier. Crucially it stays keyed on skew, so it brakes only the peer that
//! is genuinely *ahead* and releases the instant the lead falls back. Braking
//! on the absolute lead instead would fire on *both* peers whenever both leads
//! ran high — and two sims braking in lockstep can't reduce a lead that is
//! *relative*; each one's slowdown throttles its own input production, which
//! throttles the other's confirmation, pinning both leads while the framerate
//! collapses to the floor. (A peer that goes fully silent still creeps at the
//! floor; the round's stall watchdog tears that one down on a timeout.)

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
/// Emergency-brake gain: fps shaved per tick of positive skew once the lead is
/// past the high-water mark. Steep, so a peer running well below realtime is
/// matched within a few ticks of skew rather than over the steady controller's
/// multi-second onset.
const STALL_GAIN: f32 = 3.0;

/// Frames of headroom below the overflow cap that the emergency brake needs to
/// keep a genuinely *silent* peer from overrunning it before the caller's
/// watchdog fires after `timeout_secs`.
///
/// A silent peer's skew climbs with its own runaway lead, so the brake floors
/// at `MIN_STALL_FPS` on the frame the lead crosses the high-water mark; the
/// only growth left before the watchdog fires is the frozen frontier creeping
/// at the floor for the grace window. So the room the caller must leave is that
/// creep, `MIN_STALL_FPS * timeout` — a high-water mark of
/// `cap - stall_headroom(timeout)` keeps the lead under the cap for the whole
/// grace, so the recoverable timeout always beats the unrecoverable overflow
/// bail. Rounded up by a frame (there is no `const` `ceil`) to keep it strict.
pub(super) const fn stall_headroom(timeout_secs: f32) -> usize {
    (MIN_STALL_FPS * timeout_secs) as usize + 1
}

/// Emergency time-sync brake, layered over the steady-state controller in
/// [`Throttler::step`].
///
/// `lead` is the raw unmatched-local count and `high_water` the lead at which
/// this engages (both owned by the caller around [`Throttler::new`]); `skew` is
/// the raw `local - remote` advantage, the same signal the steady controller
/// smooths. Below `high_water` this is silent. Past it the steady controller's
/// [`MAX_SLOWDOWN`] cap is lifted and the slowdown ramps with the *positive*
/// skew at [`STALL_GAIN`], up to a near-full stall (`EXPECTED_FPS -
/// MIN_STALL_FPS`).
///
/// Keying the magnitude on skew rather than on the lead is what makes this safe.
/// Only the peer actually ahead (`skew > 0`) hard-brakes, so it slows toward the
/// slower peer and releases as the lead converges — self-correcting. A brake
/// keyed on the absolute lead would fire on *both* peers whenever both leads ran
/// high, and two sims braking in lockstep cannot shrink a *relative* lead: each
/// one's slowdown throttles its own input production, hence the other's
/// confirmation, so the leads stay pinned while confirmation still trickles and
/// the watchdog never fires — a self-sustaining floor-locked wedge. Using the
/// *raw* skew (not the smoothed one) also lets it react the same frame, which
/// the small overflow cap needs.
fn emergency_slowdown(skew: i32, lead: i32, high_water: i32) -> f32 {
    // Engages *at* the mark (matching the round's watchdog arming), so there's no
    // boundary frame running at full speed before the brake bites — the headroom
    // reserved by `stall_headroom` assumes the floor takes hold immediately.
    if lead < high_water {
        return 0.0;
    }
    (STALL_GAIN * skew.max(0) as f32).clamp(0.0, super::EXPECTED_FPS - MIN_STALL_FPS)
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
    /// `lead` is the raw unmatched-local count: past the high-water mark supplied
    /// to [`new`](Self::new) it lifts the `MAX_SLOWDOWN` cap and lets the
    /// emergency brake ([`emergency_slowdown`]) drive the result from the raw
    /// `skew` up to `EXPECTED_FPS - MIN_STALL_FPS`, braking a runaway the smoother
    /// can't. The final result is the larger of the two (0 = run at full speed).
    pub(crate) fn step(&mut self, skew: i32, speculation_balance: i32, lead: i32) -> f32 {
        // Computed from the raw (pre-smoothing) skew, so it engages the same
        // frame the lead crosses the mark.
        let emergency = emergency_slowdown(skew, lead, self.high_water);
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
        steady.max(emergency)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A speculation balance deep past the boundary, so the engagement gate
    /// never binds and the smoothed skew alone shapes the output.
    const OPEN: i32 = 100;

    /// Representative high-water mark for the constructed throttler — a stand-in
    /// for the real [`STALL_HIGH_WATER`](super::super::round::STALL_HIGH_WATER)
    /// (which the round derives from its overflow cap and stall timeout), kept as
    /// a local literal so these tests don't depend on the round's policy.
    const HW: i32 = 109;

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

    /// Below the high-water gate the emergency brake contributes nothing, no
    /// matter how large the skew; the steady-state cap still holds. (It engages
    /// exactly *at* the mark — see [`emergency_only_brakes_the_faster_side`].)
    #[test]
    fn emergency_gated_off_below_high_water() {
        assert_eq!(emergency_slowdown(1_000, HW - 1, HW), 0.0);
        let mut t = Throttler::new(HW);
        let peak = (0..5_000).map(|_| t.step(1_000, OPEN, HW - 1)).fold(0.0f32, f32::max);
        assert_eq!(peak, MAX_SLOWDOWN, "emergency leaked below the gate");
    }

    /// The point of keying the magnitude on skew: past the gate the brake fires
    /// only on the peer that is actually ahead. A peer that is even or behind
    /// (`skew <= 0`) is never hard-braked — which is what stops two high-lead
    /// sims from braking each other into a floor-locked wedge.
    #[test]
    fn emergency_only_brakes_the_faster_side() {
        let past = HW + 50;
        assert_eq!(emergency_slowdown(0, past, HW), 0.0);
        assert_eq!(emergency_slowdown(-30, past, HW), 0.0);
        // Ahead: ramps with the positive skew at STALL_GAIN, up to the floor.
        assert_eq!(emergency_slowdown(5, past, HW), STALL_GAIN * 5.0);
        let floor = super::super::EXPECTED_FPS - MIN_STALL_FPS;
        assert_eq!(emergency_slowdown(1_000, past, HW), floor);
    }

    /// Past the gate, a large positive skew drives the slowdown well above the
    /// steady `MAX_SLOWDOWN` cap, down to the near-full-stall floor.
    #[test]
    fn emergency_overrides_steady_cap() {
        let mut t = Throttler::new(HW);
        let out = t.step(1_000, OPEN, HW + 50);
        assert_eq!(out, super::super::EXPECTED_FPS - MIN_STALL_FPS);
    }

    /// The headroom [`stall_headroom`] reserves outlasts the watchdog for the one
    /// case it must: a peer that goes silent the instant the lead reaches a
    /// high-water mark sized as `cap - stall_headroom(timeout)`. Its skew tracks
    /// its own runaway lead, so the brake floors immediately and the frozen
    /// frontier only creeps; the lead stays under the cap for the whole grace, so
    /// the watchdog timeout fires before the overflow bail.
    #[test]
    fn reserved_headroom_outlasts_the_watchdog() {
        let timeout_secs = 5.0;
        let cap = 200i32;
        let high_water = cap - stall_headroom(timeout_secs) as i32;
        let mut t = Throttler::new(high_water);
        // Silent peer: confirmation frozen, so skew tracks the growing lead (the
        // peer's last-reported advantage is fixed, here 0). Each frame the brake
        // sets the fps, the frontier advances a tick, and confirm never does.
        let mut lead = high_water;
        let mut elapsed = 0.0f32;
        while elapsed < timeout_secs {
            let fps = super::super::EXPECTED_FPS - t.step(lead, OPEN, lead);
            elapsed += 1.0 / fps;
            lead += 1;
            assert!(lead < cap, "lead {lead} reached cap {cap} before the {timeout_secs}s watchdog");
        }
    }
}
