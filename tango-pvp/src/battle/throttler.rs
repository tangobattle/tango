//! Time-sync throttler. Converts the engine's raw per-frame skew
//! (`local_advantage - remote_advantage`) into a slowdown in fps below the base
//! rate; [`Round`](super::Round) turns that into an absolute fps target for the
//! live core. Only the leading peer slows down — the trailing peer runs at full
//! rate and lets the leader ease back toward it.

/// EMA weight applied while skew is growing. τ ≈ 5 s rise (at 60 Hz).
const ALPHA_SLOWDOWN: f32 = 1.0 / 300.0;
/// EMA weight applied while skew is shrinking. τ ≈ 0.5 s fall (at 60 Hz).
const ALPHA_SPEEDUP: f32 = 1.0 / 30.0;
/// Slowdown ceiling, in fps below the base rate.
const MAX_SLOWDOWN: f32 = 30.0;

/// Per-round time-sync throttler. [`Round`](super::Round) owns one and feeds it
/// the engine's raw skew each frame.
pub(crate) struct Throttler {
    /// Asymmetric-EMA-smoothed skew, carried across frames. Floored at zero:
    /// negative skew (the peer leading) would otherwise wind the average down —
    /// the fast fall digs the hole in under a second, and the slow rise then
    /// spends 10+ seconds climbing back out before a new lead of ours gets
    /// throttled at all. Above zero the value stays raw past MAX_SLOWDOWN; the
    /// fast fall drains any overshoot in a few hundred ms, so clamping there
    /// would buy nothing.
    smoothed: f32,
}

impl Throttler {
    pub(crate) fn new() -> Self {
        Self { smoothed: 0.0 }
    }

    /// Compute the slowdown to apply this frame, in fps below the base rate.
    /// `skew` is the raw integer frame difference
    /// `local_advantage - remote_advantage`. `headroom` is how many
    /// speculation-free frames the present delay still absorbs (the engine's
    /// `(-speculation_balance).max(0)`): while the lead fits inside the present
    /// delay, running ahead costs no presentation quality, so that much of the
    /// smoothed skew is forgiven before any throttling. The result is always in
    /// `[0, MAX_SLOWDOWN]` (0 = run at full speed).
    pub(crate) fn step(&mut self, skew: i32, headroom: f32) -> f32 {
        let skew = skew as f32;
        let alpha = if skew > self.smoothed {
            ALPHA_SLOWDOWN
        } else {
            ALPHA_SPEEDUP
        };
        self.smoothed = (alpha * skew + (1.0 - alpha) * self.smoothed).max(0.0);
        (self.smoothed - headroom).clamp(0.0, MAX_SLOWDOWN)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(t: &mut Throttler, skew: i32, frames: u32) {
        for _ in 0..frames {
            t.step(skew, 0.0);
        }
    }

    /// Frames of sustained `skew` until the emitted slowdown reaches `target`.
    fn frames_until(t: &mut Throttler, skew: i32, target: f32) -> u32 {
        for n in 1..=10_000 {
            if t.step(skew, 0.0) >= target {
                return n;
            }
        }
        panic!("slowdown never reached {target}");
    }

    /// Sustained lead engages the brake on the designed slow rise: skew held
    /// at 20 crosses a 10 fps slowdown in ≈208 frames (~3.5 s at 60 Hz).
    #[test]
    fn onset_is_gentle() {
        let n = frames_until(&mut Throttler::new(), 20, 10.0);
        assert!((190..=225).contains(&n), "engaged after {n} frames");
    }

    /// Once skew settles back to zero, the fast fall lets the brake go in
    /// under a second.
    #[test]
    fn release_is_fast() {
        let mut t = Throttler::new();
        let mut n = 0;
        while t.step(120, 0.0) < MAX_SLOWDOWN {
            n += 1;
            assert!(n < 1_000, "never hit the cap");
        }
        let mut n = 0;
        while t.step(0, 0.0) > 5.0 {
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
        let fresh = frames_until(&mut Throttler::new(), 20, 10.0);
        let mut t = Throttler::new();
        run(&mut t, -120, 300);
        let after_history = frames_until(&mut t, 20, 10.0);
        assert_eq!(
            after_history, fresh,
            "negative windup deferred onset: {after_history} frames vs {fresh} fresh"
        );
    }

    /// The lead inside the present delay is forgiven before any throttling.
    #[test]
    fn headroom_is_forgiven() {
        let mut t = Throttler::new();
        run(&mut t, 10, 5_000);
        assert!(t.step(10, 0.0) > 9.0);
        assert!(t.step(10, 10.0) < 0.01);
    }

    #[test]
    fn slowdown_is_capped() {
        let mut t = Throttler::new();
        let peak = (0..5_000).map(|_| t.step(1_000, 0.0)).fold(0.0f32, f32::max);
        assert_eq!(peak, MAX_SLOWDOWN);
    }
}
