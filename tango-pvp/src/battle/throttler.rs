//! FPS-target throttler strategies. Each takes the current raw skew
//! (`local_advantage - remote_advantage`, in frames) and returns a
//! slowdown amount in fps; the caller applies it as
//! `EXPECTED_FPS - slowdown`. Only the leading peer corrects —
//! trailers run at full speed and rely on the leader pulling back.
//! Each strategy is wrapped in [`Clamp`] at the factory level to
//! enforce a uniform worst-case ceiling on audio warp.
//!
//! Strategies are trait objects so the active one can be swapped at
//! runtime (e.g. via a debug menu); each impl owns both its tuning
//! parameters and its per-round mutable state, so swapping mid-round
//! resets cleanly.

/// A per-round throttler.
pub trait Throttler: Send {
    /// Compute the slowdown to apply this frame, in fps below
    /// `EXPECTED_FPS`. `skew` is the raw integer frame difference
    /// `local_advantage - remote_advantage`. Typically non-negative
    /// (0 = run at full speed); signed strategies (e.g. [`Power`])
    /// can return negative values to request a speed-up. Wrap with
    /// [`Clamp`] to bound the result against a worst-case ceiling.
    fn step(&mut self, skew: i32) -> f32;
}

/// Adapter that bounds any inner throttler's output to the `[min,
/// max]` range, in fps. Lives here (not in `round.rs`) so callers
/// can pick a uniform ceiling at factory time and round.rs doesn't
/// have to know about clamping. Negative bounds mean a speed-up
/// limit; positive bounds mean a slowdown limit.
pub struct Clamp<T> {
    pub min: f32,
    pub max: f32,
    inner: T,
}

impl<T: Throttler> Clamp<T> {
    /// Build with an explicit range. Negative `min` allows the
    /// inner throttler to request speed-ups; positive `max` caps
    /// the slowdown side.
    pub fn new(inner: T, min: f32, max: f32) -> Self {
        Self { min, max, inner }
    }
}

impl<T: Default + Throttler> Default for Clamp<T> {
    /// Slowdown ceiling at 30 fps (half of `EXPECTED_FPS`,
    /// bounding the worst-case audio warp at 2×); speed-up side
    /// is unbounded so an inner strategy that requests aggressive
    /// catch-up isn't artificially limited.
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

/// Continuous proportional throttler smoothed by an asymmetric EMA on
/// skew. `alpha_slowdown` is used when skew is growing (smoothed value
/// climbs gradually, so sub-second loss bursts don't engage the
/// throttler); `alpha_speedup` is used when skew is shrinking (smoothed
/// value drops fast, so the throttler lifts as soon as the imbalance
/// closes). Net: gentle glide into a slowdown, snappy return out of it.
pub struct AsymmetricEma {
    pub alpha_slowdown: f32,
    pub alpha_speedup: f32,
    smoothed: f32,
}

impl AsymmetricEma {
    /// Default tuning: τ ≈ 5 s rise, τ ≈ 0.5 s fall.
    pub fn new() -> Self {
        Self {
            // τ ≈ 5 s @ 60 Hz — a 0.5 s spike of raw skew +30
            // contributes ~30·(1-exp(-0.5/5)) ≈ +2.9 to the smoothed
            // value, so the throttler barely moves under bursty loss.
            alpha_slowdown: 1.0 / 300.0,
            // τ ≈ 0.5 s @ 60 Hz — once the imbalance closes the
            // local fps returns to 60 within a handful of frames.
            alpha_speedup: 1.0 / 30.0,
            smoothed: 0.0,
        }
    }
}

impl Default for AsymmetricEma {
    fn default() -> Self {
        Self::new()
    }
}

impl Throttler for AsymmetricEma {
    fn step(&mut self, skew: i32) -> f32 {
        let skew = skew as f32;
        let alpha = if skew > self.smoothed {
            self.alpha_slowdown
        } else {
            self.alpha_speedup
        };
        self.smoothed = alpha * skew + (1.0 - alpha) * self.smoothed;
        self.smoothed.max(0.0)
    }
}

/// Pure-linear slowdown: `slope · skew` for positive skew, 0 otherwise.
/// Stateless. On its own this would react instantly to every frame of
/// jitter; pair it with [`Watchdog`] to gate it behind a deadband and
/// sustained-frame counter.
pub struct Linear {
    pub slope: f32,
}

impl Linear {
    pub fn new() -> Self {
        Self { slope: 1.0 }
    }
}

impl Default for Linear {
    fn default() -> Self {
        Self::new()
    }
}

impl Throttler for Linear {
    fn step(&mut self, skew: i32) -> f32 {
        if skew <= 0 {
            0.0
        } else {
            skew as f32 * self.slope
        }
    }
}

/// Gates an inner throttler behind a deadband + sustained-frame
/// counter. Returns 0 until raw skew has been above `threshold` for
/// `trigger_frames` consecutive frames; while engaged, returns
/// whatever the inner throttler says. Resets the trigger counter the
/// first frame skew dips back under the threshold, so bursty loss
/// spikes (which resolve faster than the trigger) never engage.
///
/// Composes with any [`Throttler`] — wrap `Linear` to get the classic
/// deadband + linear-slowdown behavior, or wrap an EMA to combine the
/// deadband with smoother engagement.
pub struct Watchdog<T> {
    pub threshold: i32,
    pub trigger_frames: u32,
    inner: T,
    sustained: u32,
}

impl<T: Throttler> Watchdog<T> {
    /// Default tuning: 2-frame deadband, 60-frame trigger (~1 s).
    pub fn new(inner: T) -> Self {
        Self {
            threshold: 2,
            trigger_frames: 60,
            inner,
            sustained: 0,
        }
    }
}

impl<T: Default + Throttler> Default for Watchdog<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: Throttler> Throttler for Watchdog<T> {
    fn step(&mut self, skew: i32) -> f32 {
        if skew > self.threshold {
            self.sustained = self.sustained.saturating_add(1);
        } else {
            self.sustained = 0;
        }
        if self.sustained > self.trigger_frames {
            self.inner.step(skew)
        } else {
            0.0
        }
    }
}

/// Symmetric power-law throttler on instantaneous skew. Matches tango
/// v4.x's `dtick`-based tuning: at |skew| = `knee` the slowdown is
/// exactly 1 fps, below it the curve falls off sharply (implicit
/// deadband), above it it grows super-linearly so big rifts close
/// fast. Stateless — each frame's decision depends only on the
/// current skew.
pub struct Power {
    pub knee: f32,
    pub exponent: f32,
}

impl Power {
    /// Default tuning: knee at 15-frame skew, exponent 7/3.
    pub fn new() -> Self {
        Self {
            knee: 15.0,
            exponent: 7.0 / 3.0,
        }
    }
}

impl Default for Power {
    fn default() -> Self {
        Self::new()
    }
}

impl Throttler for Power {
    fn step(&mut self, skew: i32) -> f32 {
        (skew.abs() as f32 / self.knee)
            .powf(self.exponent)
            .copysign(skew as f32)
    }
}
