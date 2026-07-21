//! The fixed-timestep accumulator that replaces the native client's
//! sleeping `Pacer`: each pump integrates elapsed wall time at the
//! session's (possibly fractional) fps target and returns how many
//! whole ticks are due, bounded so a stall becomes a resync instead of
//! a sprint.

/// Bounds worst-case pump time when catching up after a throttled or
/// missed callback (hidden tab, GC pause).
pub const MAX_TICKS_PER_PUMP: u32 = 6;

/// Gaps longer than this are a stall: resync the cadence rather than
/// racing to make up lost frames (mirrors the native Pacer's rule).
const STALL_SECS: f64 = 0.25;

pub struct TickClock {
    last_ms: Option<f64>,
    /// Fractional ticks owed.
    owed: f64,
}

impl TickClock {
    pub fn new() -> TickClock {
        TickClock {
            last_ms: None,
            owed: 0.0,
        }
    }

    /// Drop the cadence (pause ended, session swapped, boot finished).
    pub fn reset(&mut self) {
        self.last_ms = None;
        self.owed = 0.0;
    }

    /// Advance to `now_ms`; return the whole ticks due at `fps`
    /// (0.0 = paused), capped at [`MAX_TICKS_PER_PUMP`].
    ///
    /// Fractional targets (the netplay throttler asks for e.g. 59.3)
    /// are honored by construction — the accumulator integrates
    /// `elapsed × fps` in tick units, so 59.3 yields 59 ticks one
    /// second and 60 the next.
    pub fn due(&mut self, now_ms: f64, fps: f32) -> u32 {
        let Some(last) = self.last_ms.replace(now_ms) else {
            return 0;
        };
        if fps <= 0.0 {
            self.owed = 0.0;
            return 0;
        }
        let mut dt = (now_ms - last) / 1000.0;
        if dt > STALL_SECS {
            self.owed = 0.0;
            dt = 1.0 / 60.0;
        }
        self.owed += dt * fps as f64;
        let due = (self.owed.floor() as u32).min(MAX_TICKS_PER_PUMP);
        // Forgive debt past the cap — never let a long pause turn into
        // a sprint that fights the throttler.
        self.owed = (self.owed - due as f64).min(1.0);
        due
    }
}
