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
