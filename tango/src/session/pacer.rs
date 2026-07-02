//! Wall-clock frame pacing for live emulator sessions.
//!
//! Sessions run their cores with mGBA's sync-to-audio OFF and pace the
//! emulator thread themselves: each session's frame callback calls
//! [`Pacer::pace`] with the sync's current `fps_target`, and the pacer
//! sleeps out the remainder of the frame period. The audio device is a
//! best-effort tap of the core's sample ring (see [`crate::audio`]),
//! never a pacing dependency — a stalled or misbehaving output device
//! (virtual audio cables, sleeping Bluetooth headsets) costs audio, not
//! emulation progress. Under netplay that distinction is the difference
//! between "sound cut out" and "the match froze and the opponent got a
//! phantom disconnect".

/// How many frames behind schedule the pacer will run flat-out to catch
/// up before concluding its schedule is stale and rebasing on the
/// present. Small overshoots (an oversleep, one long frame) are repaid
/// so the average rate holds; a long gap means frames stopped entirely
/// (pause, seek chase) and sprinting to make it up would fast-forward.
const MAX_CATCHUP_FRAMES: u32 = 3;

pub struct Pacer {
    /// Wall-clock deadline the in-progress frame should complete at.
    /// `None` until the first paced frame. A Cell because the frame
    /// callback holding the pacer is a `Fn` closure — access is
    /// emulator-thread-only, but only through a shared reference.
    next_deadline: std::cell::Cell<Option<std::time::Instant>>,
}

impl Pacer {
    pub fn new() -> Self {
        Self {
            next_deadline: std::cell::Cell::new(None),
        }
    }

    /// Sleep out the remainder of the current frame's period so frames
    /// complete at `fps_target` per second on average. Call once per
    /// frame, on the emulator thread, for realtime frames only —
    /// callers skip it for frames a seek chase or rewind backfill
    /// drives, which should run as fast as possible.
    pub fn pace(&self, fps_target: f32) {
        if !fps_target.is_finite() || fps_target <= 0.0 {
            self.next_deadline.set(None);
            return;
        }
        let period = std::time::Duration::from_secs_f64(1.0 / f64::from(fps_target));
        let now = std::time::Instant::now();
        let deadline = match self.next_deadline.get() {
            // On schedule: sleep off the rest of the frame.
            Some(d) if d > now => {
                std::thread::sleep(d - now);
                d
            }
            // Slightly behind: no sleep, but keep the original cadence
            // so the shortfall is repaid over the next few frames.
            Some(d) if now - d < period * MAX_CATCHUP_FRAMES => d,
            // Fresh start, or so far behind the schedule is stale.
            _ => now,
        };
        self.next_deadline.set(Some(deadline + period));
    }

    /// Read the thread-sync `fps_target` off `core` and pace by it. The
    /// sync field is the single source of truth for session speed —
    /// every speed control (fast-forward, replay speed, the PvP
    /// time-sync throttler) writes it there.
    pub fn pace_by_sync_target(&self, core: &mut mgba::core::CoreMutRef<'_>) {
        let mut gba = core.gba_mut();
        let Some(sync) = gba.sync_mut() else {
            return;
        };
        self.pace(sync.as_ref().fps_target());
    }
}

impl Default for Pacer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Lower bounds only: sleeps are guaranteed minimums, while upper
    // bounds are at the mercy of CI scheduling.

    #[test]
    fn holds_target_rate() {
        let pacer = Pacer::new();
        let start = std::time::Instant::now();
        for _ in 0..25 {
            pacer.pace(240.0);
        }
        // 24 full periods after the first call's rebase.
        assert!(start.elapsed() >= std::time::Duration::from_secs_f64(24.0 / 240.0));
    }

    #[test]
    fn rebases_after_stall_instead_of_sprinting() {
        let pacer = Pacer::new();
        pacer.pace(60.0);
        // Simulate frames stopping for far longer than the catch-up
        // window (a pause, a seek chase).
        std::thread::sleep(std::time::Duration::from_millis(150));
        let start = std::time::Instant::now();
        for _ in 0..4 {
            pacer.pace(60.0);
        }
        // The stalled schedule must be rebased, not repaid: the first
        // call is free, the next three each sleep a full period.
        assert!(start.elapsed() >= std::time::Duration::from_secs_f64(3.0 / 60.0));
    }

    #[test]
    fn non_finite_target_disables_pacing() {
        let pacer = Pacer::new();
        pacer.pace(60.0);
        let start = std::time::Instant::now();
        pacer.pace(f32::INFINITY);
        pacer.pace(0.0);
        assert!(start.elapsed() < std::time::Duration::from_millis(10));
    }
}
