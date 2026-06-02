//! Frame-rate-target throttler. Takes the current raw skew
//! (`local_advantage - remote_advantage`, in frames) and returns a slowdown
//! amount in fps; the driver applies it as `base_rate - slowdown`. Only the
//! leading peer corrects — trailers run at full speed and rely on the leader
//! pulling back. [`Ema`] is wrapped in [`Clamp`] to enforce a uniform
//! worst-case ceiling on how far the rate can be warped.

/// A per-round throttler.
pub trait Throttler: Send {
    /// Compute the slowdown to apply this frame, in fps below the base rate.
    /// `skew` is the raw integer frame difference
    /// `local_advantage - remote_advantage`. Typically non-negative (0 = run
    /// at full speed); the underlying [`Ema`] is signed and can return
    /// negative values to request a speed-up. Wrap with [`Clamp`] to bound the
    /// result.
    fn step(&mut self, skew: i32) -> f32;
}

/// Adapter that bounds the inner throttler's output to the `[min, max]` range,
/// in fps. Negative bounds mean a speed-up limit; positive bounds mean a
/// slowdown limit.
pub struct Clamp<T> {
    pub min: f32,
    pub max: f32,
    inner: T,
}

impl<T: Throttler> Clamp<T> {
    /// Override the lower bound, returning `self` for chaining.
    pub fn with_min(mut self, min: f32) -> Self {
        self.min = min;
        self
    }

    /// Override the upper bound, returning `self` for chaining.
    pub fn with_max(mut self, max: f32) -> Self {
        self.max = max;
        self
    }
}

impl<T: Default + Throttler> Default for Clamp<T> {
    /// Slowdown ceiling at 30 fps; speed-up side is unbounded so an inner
    /// strategy that requests aggressive catch-up isn't artificially limited.
    fn default() -> Self {
        Self {
            min: f32::NEG_INFINITY,
            max: 30.0,
            inner: T::default(),
        }
    }
}

impl<T: Throttler> Throttler for Clamp<T> {
    fn step(&mut self, skew: i32) -> f32 {
        self.inner.step(skew).clamp(self.min, self.max)
    }
}

/// Continuous proportional throttler smoothed by an asymmetric EMA on skew.
/// `alpha_slowdown` is used when skew is growing (smoothed value climbs
/// gradually, so sub-second loss bursts don't engage the throttler);
/// `alpha_speedup` is used when skew is shrinking (smoothed value drops fast,
/// so the throttler lifts as soon as the imbalance closes). Net: gentle glide
/// into a slowdown, snappy return out of it. Returns the raw smoothed value,
/// including negatives — wrap with [`Clamp`] (`with_min(0.0)`) to gate
/// speed-ups.
pub struct Ema {
    pub alpha_slowdown: f32,
    pub alpha_speedup: f32,
    smoothed: f32,
}

impl Ema {
    /// Default tuning: τ ≈ 5 s rise, τ ≈ 0.5 s fall (at 60 Hz).
    pub fn new() -> Self {
        Self {
            alpha_slowdown: 1.0 / 300.0,
            alpha_speedup: 1.0 / 30.0,
            smoothed: 0.0,
        }
    }
}

impl Default for Ema {
    fn default() -> Self {
        Self::new()
    }
}

impl Throttler for Ema {
    fn step(&mut self, skew: i32) -> f32 {
        let skew = skew as f32;
        let alpha = if skew > self.smoothed {
            self.alpha_slowdown
        } else {
            self.alpha_speedup
        };
        self.smoothed = alpha * skew + (1.0 - alpha) * self.smoothed;
        self.smoothed
    }
}
