//! FPS-target throttler strategies. Each takes the current raw skew
//! (`local_advantage - remote_advantage`, in frames) and returns a
//! non-negative slowdown amount in fps; the caller clamps it against
//! a global max and applies it as `EXPECTED_FPS - slowdown`. Only the
//! leading peer corrects — trailers run at full speed and rely on the
//! leader pulling back.
//!
//! Strategies are trait objects so the active one can be swapped at
//! runtime (e.g. via a debug menu); each impl owns both its tuning
//! parameters and its per-round mutable state, so swapping mid-round
//! resets cleanly.

/// A per-round throttler.
pub trait Throttler: Send {
    /// Compute the slowdown to apply this frame, in fps below
    /// `EXPECTED_FPS`. Non-negative; 0 means run at full speed. The
    /// caller is responsible for capping against any global maximum.
    fn step(&mut self, skew: f32) -> f32;
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
    fn step(&mut self, skew: f32) -> f32 {
        let alpha = if skew > self.smoothed {
            self.alpha_slowdown
        } else {
            self.alpha_speedup
        };
        self.smoothed = alpha * skew + (1.0 - alpha) * self.smoothed;
        self.smoothed.max(0.0)
    }
}

/// Idle-until-tripped throttler: a sustained-frame counter climbs while
/// raw skew is above `threshold` and resets to zero otherwise; once it
/// crosses `trigger_frames`, a linear slowdown proportional to current
/// skew engages. The deadband + trigger combo rejects bursty packet-
/// loss spikes (which resolve faster than the trigger).
pub struct LinearWatchdog {
    pub threshold: f32,
    pub trigger_frames: u32,
    sustained: u32,
}

impl LinearWatchdog {
    /// Default tuning: 2-frame deadband, 60-frame trigger (~1 s).
    pub fn new() -> Self {
        Self {
            threshold: 2.0,
            trigger_frames: 60,
            sustained: 0,
        }
    }
}

impl Default for LinearWatchdog {
    fn default() -> Self {
        Self::new()
    }
}

impl Throttler for LinearWatchdog {
    fn step(&mut self, skew: f32) -> f32 {
        if skew > self.threshold {
            self.sustained = self.sustained.saturating_add(1);
        } else {
            self.sustained = 0;
        }
        if self.sustained > self.trigger_frames {
            skew.max(0.0)
        } else {
            0.0
        }
    }
}

/// Asymmetric power-law throttler on instantaneous skew. Matches tango
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
    fn step(&mut self, skew: f32) -> f32 {
        if skew <= 0.0 {
            0.0
        } else {
            (skew / self.knee).powf(self.exponent)
        }
    }
}
