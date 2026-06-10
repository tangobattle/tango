//! Time-sync throttler. Converts the engine's raw per-frame skew
//! (`local_advantage - remote_advantage`) into a slowdown in fps below the base
//! rate; [`Round`](super::Round) turns that into an absolute fps target for the
//! live core. Only the leading peer slows down — the trailing peer runs at full
//! rate and lets the leader ease back toward it. Throttling also only engages
//! once the presented frame actually speculates past the present delay: while
//! the lead still fits inside it, running ahead costs no presentation quality,
//! so no fps is shaved.

/// EMA weight applied while skew is growing. τ ≈ 5 s rise (at 60 Hz).
const ALPHA_SLOWDOWN: f32 = 1.0 / 300.0;
/// EMA weight applied while skew is shrinking. τ ≈ 0.5 s fall (at 60 Hz).
const ALPHA_SPEEDUP: f32 = 1.0 / 30.0;
/// EMA weight for the speculation balance. τ ≈ 0.5 s (at 60 Hz): the raw
/// balance moves in bursts as remote inputs arrive, and smoothing it keeps
/// that flutter out of the emitted fps target.
const ALPHA_BALANCE: f32 = 1.0 / 30.0;
/// Engagement ramp at the speculation boundary: fps of slowdown permitted per
/// tick of smoothed speculative depth. Deliberately steep — the ramp decides
/// *when* throttling engages (at the boundary, continuously), not how hard;
/// magnitude is still the smoothed skew's job, which takes over within the
/// first few ticks of depth.
const ENGAGEMENT_SLOPE: f32 = 10.0;
/// Slowdown ceiling, in fps below the base rate.
const MAX_SLOWDOWN: f32 = 30.0;

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
    /// value keeps that flutter out of the fps target.
    smoothed_balance: f32,
}

impl Throttler {
    pub(crate) fn new() -> Self {
        Self {
            smoothed: 0.0,
            smoothed_balance: 0.0,
        }
    }

    /// Compute the slowdown to apply this frame, in fps below the base rate.
    ///
    /// `skew` is the raw integer frame difference
    /// `local_advantage - remote_advantage`; its asymmetric EMA sets the
    /// slowdown magnitude. `speculation_balance` is the engine's signed
    /// distance of the presented frame from the speculation boundary
    /// (`lead - present_delay`); its smoothed sign gates engagement. While the
    /// smoothed balance is negative the presented frame is fully confirmed —
    /// running ahead costs no presentation quality — so the result is 0 no
    /// matter how large the skew. Past the boundary the permitted slowdown
    /// ramps up at ENGAGEMENT_SLOPE fps per tick of smoothed depth until the
    /// smoothed skew takes over. The result is always in `[0, MAX_SLOWDOWN]`
    /// (0 = run at full speed).
    pub(crate) fn step(&mut self, skew: i32, speculation_balance: i32) -> f32 {
        let skew = skew as f32;
        let alpha = if skew > self.smoothed {
            ALPHA_SLOWDOWN
        } else {
            ALPHA_SPEEDUP
        };
        self.smoothed = (alpha * skew + (1.0 - alpha) * self.smoothed).max(0.0);
        self.smoothed_balance =
            ALPHA_BALANCE * speculation_balance as f32 + (1.0 - ALPHA_BALANCE) * self.smoothed_balance;
        self.smoothed
            .min(ENGAGEMENT_SLOPE * self.smoothed_balance.max(0.0))
            .clamp(0.0, MAX_SLOWDOWN)
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
}
