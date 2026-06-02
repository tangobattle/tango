//! Frame-rate-target throttler. Takes the current raw skew
//! (`local_advantage - remote_advantage`, in frames) and returns a slowdown
//! amount in fps; the driver applies it as `base_rate - slowdown`. Only the
//! leading peer corrects — trailers run at full speed and rely on the leader
//! pulling back.
//!
//! The strategy is a continuous proportional response smoothed by an asymmetric
//! EMA on skew, with the result clamped to a uniform worst-case window:
//!
//! - [`ALPHA_SLOWDOWN`] is used when skew is growing (the smoothed value climbs
//!   gradually, so sub-second loss bursts don't engage the throttler);
//! - [`ALPHA_SPEEDUP`] is used when skew is shrinking (the smoothed value drops
//!   fast, so the throttler lifts as soon as the imbalance closes).
//!
//! Net: a gentle glide into a slowdown, a snappy return out of it. The smoothed
//! value is then clamped to `[0, MAX_SLOWDOWN]` fps — negatives (speed-up
//! requests) are gated off, and the slowdown is capped so the rate can't be
//! warped past a uniform worst-case ceiling.

/// EMA weight applied while skew is growing. τ ≈ 5 s rise (at 60 Hz).
const ALPHA_SLOWDOWN: f32 = 1.0 / 300.0;
/// EMA weight applied while skew is shrinking. τ ≈ 0.5 s fall (at 60 Hz).
const ALPHA_SPEEDUP: f32 = 1.0 / 30.0;
/// Slowdown ceiling, in fps below the base rate.
const MAX_SLOWDOWN: f32 = 30.0;

/// The engine's per-session time-sync throttler. The [`Session`](crate::Session)
/// owns one and feeds it the raw skew every frame; the host never configures it.
pub(crate) struct Throttler {
    /// Asymmetric-EMA-smoothed skew, carried across frames. Holds the raw
    /// (unclamped) value so the clamp only shapes the emitted slowdown, not the
    /// running average.
    smoothed: f32,
}

impl Throttler {
    pub(crate) fn new() -> Self {
        Self { smoothed: 0.0 }
    }

    /// Compute the slowdown to apply this frame, in fps below the base rate.
    /// `skew` is the raw integer frame difference
    /// `local_advantage - remote_advantage`. The result is always in
    /// `[0, MAX_SLOWDOWN]` (0 = run at full speed).
    pub(crate) fn step(&mut self, skew: i32) -> f32 {
        let skew = skew as f32;
        let alpha = if skew > self.smoothed {
            ALPHA_SLOWDOWN
        } else {
            ALPHA_SPEEDUP
        };
        self.smoothed = alpha * skew + (1.0 - alpha) * self.smoothed;
        self.smoothed.clamp(0.0, MAX_SLOWDOWN)
    }
}
