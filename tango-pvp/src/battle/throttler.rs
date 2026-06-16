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
//! The skew is filtered with a sliding **median**, not an average. Under packet
//! loss the redundancy window recovers dropped inputs in clumps, so the raw
//! skew spikes every time a clump lands; a median throws those spikes out as the
//! minority they are, where a mean (or EMA) chases them and the slowdown
//! oscillates. The median is then *halved* before it drives the slowdown
//! ([`SKEW_TO_SLOWDOWN`]) — the skew already double-counts the offset.

use std::collections::VecDeque;

/// Sliding-window length for the skew median, in frames (~0.25 s at 60 Hz).
/// Odd, so the median is an unambiguous middle element. Long enough that
/// loss-recovery spikes stay a clear minority of the window (even at heavy loss
/// they're a small fraction of frames, so they can't move the median), short
/// enough to add little lag — lag here is feedback dead-time, which the loop
/// pays for in stability.
const WINDOW: usize = 15;
/// Gain from the median lead to emitted slowdown, in fps shaved per tick of
/// lead. **0.5, not 1.0**: the raw skew is `local_advantage − remote_advantage`,
/// and the one-way network delay cancels into a *doubling* of the true clock
/// offset (`skew ≈ 2 × offset` — that cancellation is the whole point of taking
/// the difference). Correcting the full skew over-drives the offset 2×, which is
/// invisible at LAN latency but rings once the round-trip dead-time is
/// non-trivial. Halving it makes the controller correct the offset itself, on a
/// ~1 s time constant — comfortably slower than any playable RTT.
const SKEW_TO_SLOWDOWN: f32 = 0.5;
/// EMA weight for the speculation balance. τ ≈ 0.5 s (at 60 Hz): the raw balance
/// moves in bursts as remote inputs arrive, and smoothing it keeps that flutter
/// out of the emitted fps target. The balance gate wants the *mean* tendency
/// near the boundary, so it stays an average — a median would snap the gate
/// fully open or shut for a balance hovering on the boundary.
const ALPHA_BALANCE: f32 = 1.0 / 30.0;
/// Engagement ramp at the speculation boundary: fps of slowdown permitted per
/// tick of smoothed speculative depth. Deliberately steep — the ramp decides
/// *when* throttling engages (at the boundary, continuously), not how hard;
/// magnitude is the median lead's job, which takes over within the first few
/// ticks of depth.
const ENGAGEMENT_SLOPE: f32 = 10.0;
/// Slowdown ceiling, in fps below the base rate.
const MAX_SLOWDOWN: f32 = 30.0;
/// Floor on the smoothed speculation balance. Deep headroom (lead pinned well
/// inside the present delay) would otherwise wind the balance EMA down toward
/// -present_delay, and the climb back through zero would defer engagement past
/// the boundary by however long the slider made it. The ramp saturates the
/// slowdown ceiling at MAX_SLOWDOWN/ENGAGEMENT_SLOPE ticks of depth, so flooring
/// at its mirror image keeps the gate's swing symmetric around the boundary and
/// the re-engagement latency bounded.
const BALANCE_FLOOR: f32 = -(MAX_SLOWDOWN / ENGAGEMENT_SLOPE);

/// Per-round time-sync throttler. [`Round`](super::Round) owns one and feeds it
/// the engine's raw skew and speculation balance each frame.
pub(crate) struct Throttler {
    /// The last [`WINDOW`] raw skews; their median is the spike-robust lead
    /// estimate that drives the slowdown.
    skew_window: VecDeque<i32>,
    /// EMA-smoothed speculation balance. The raw balance jumps frame to frame as
    /// remote inputs arrive in bursts; gating engagement on the smoothed value
    /// keeps that flutter out of the fps target. Floored at [`BALANCE_FLOOR`] so
    /// deep headroom can't wind it down and defer the next engagement.
    smoothed_balance: f32,
}

impl Throttler {
    pub(crate) fn new() -> Self {
        Self {
            skew_window: VecDeque::with_capacity(WINDOW),
            smoothed_balance: 0.0,
        }
    }

    /// Compute the slowdown to apply this frame, in fps below the base rate.
    ///
    /// `skew` is the raw integer frame difference
    /// `local_advantage − remote_advantage`; its sliding-window median, halved
    /// by [`SKEW_TO_SLOWDOWN`], sets the slowdown magnitude (a negative median —
    /// the peer leading — sheds nothing). `speculation_balance` is the engine's
    /// signed distance of the presented frame from the speculation boundary
    /// (`lead − present_delay`); its smoothed sign gates engagement. While the
    /// smoothed balance is negative the presented frame is fully confirmed —
    /// running ahead costs no presentation quality — so the result is 0 no
    /// matter the lead. Past the boundary the permitted slowdown ramps up at
    /// [`ENGAGEMENT_SLOPE`] fps per tick of smoothed depth until the median lead
    /// takes over. The result is always in `[0, MAX_SLOWDOWN]` (0 = full speed).
    pub(crate) fn step(&mut self, skew: i32, speculation_balance: i32) -> f32 {
        if self.skew_window.len() == WINDOW {
            self.skew_window.pop_front();
        }
        self.skew_window.push_back(skew);
        self.smoothed_balance = (ALPHA_BALANCE * speculation_balance as f32
            + (1.0 - ALPHA_BALANCE) * self.smoothed_balance)
            .max(BALANCE_FLOOR);
        let lead = self.skew_median().max(0.0);
        (SKEW_TO_SLOWDOWN * lead)
            .min(ENGAGEMENT_SLOPE * self.smoothed_balance.max(0.0))
            .clamp(0.0, MAX_SLOWDOWN)
    }

    /// Median of the current skew window (0 when empty). `select_nth_unstable`
    /// is linear and the window is tiny, so this is cheap per frame.
    fn skew_median(&self) -> f32 {
        if self.skew_window.is_empty() {
            return 0.0;
        }
        let mut v: Vec<i32> = self.skew_window.iter().copied().collect();
        let mid = v.len() / 2;
        let (_, m, _) = v.select_nth_unstable(mid);
        *m as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A speculation balance deep past the boundary, so the engagement gate
    /// never binds and the median lead alone shapes the output.
    const OPEN: i32 = 100;

    fn run(t: &mut Throttler, skew: i32, balance: i32, frames: u32) {
        for _ in 0..frames {
            t.step(skew, balance);
        }
    }

    /// Frames of sustained input until the emitted slowdown reaches `target`.
    fn frames_until(t: &mut Throttler, skew: i32, balance: i32, target: f32) -> u32 {
        for n in 1..=10_000 {
            if t.step(skew, balance) >= target {
                return n;
            }
        }
        panic!("slowdown never reached {target}");
    }

    /// The skew is twice the true clock offset, so the throttler corrects half
    /// of it: a steady skew of 20 sheds 10 fps, not 20.
    #[test]
    fn corrects_half_the_lead() {
        let mut t = Throttler::new();
        run(&mut t, 20, OPEN, WINDOW as u32);
        assert_eq!(t.step(20, OPEN), 10.0);
    }

    /// The point of the median: loss-recovery spikes are rejected. A steady lead
    /// of 4 with a big skew spike one frame in six (what a clump of recovered
    /// inputs looks like) keeps the slowdown pinned at 0.5 * 4 = 2 — it never
    /// chases the spike. A mean would land near 0.5 * 10 = 5 and jitter.
    #[test]
    fn recovery_spikes_are_rejected() {
        let mut t = Throttler::new();
        let sample = |i: u32| if i % 6 == 0 { 40 } else { 4 };
        let outs: Vec<f32> = (0..600).map(|i| t.step(sample(i), OPEN)).collect();
        let tail = &outs[WINDOW..];
        let hi = tail.iter().copied().fold(f32::MIN, f32::max);
        let lo = tail.iter().copied().fold(f32::MAX, f32::min);
        assert!(hi <= 3.0, "a spike leaked into the slowdown: {hi} fps");
        assert!(hi - lo <= 0.5, "slowdown fluttered by {} fps", hi - lo);
    }

    /// A large sustained lead engages promptly (the median of a uniform window
    /// is the value itself) and saturates the ceiling.
    #[test]
    fn sustained_lead_is_capped() {
        let mut t = Throttler::new();
        let peak = (0..WINDOW as u32 * 2).map(|_| t.step(1_000, OPEN)).fold(0.0f32, f32::max);
        assert_eq!(peak, MAX_SLOWDOWN);
    }

    /// Once the lead clears, the brake releases within one window: the median
    /// falls to zero as soon as the fresh zeros outvote the stale lead.
    #[test]
    fn release_within_a_window() {
        let mut t = Throttler::new();
        run(&mut t, 40, OPEN, WINDOW as u32);
        let mut n = 0;
        while t.step(0, OPEN) > 0.0 {
            n += 1;
            assert!(n <= WINDOW, "release took longer than a window: {n} frames");
        }
    }

    /// The trailing peer (negative skew) never brakes — only the leader does.
    #[test]
    fn trailing_peer_never_brakes() {
        let mut t = Throttler::new();
        run(&mut t, -20, OPEN, WINDOW as u32 * 2);
        assert_eq!(t.step(-20, OPEN), 0.0);
    }

    /// A peer-led history (negative skew) can't defer our onset by more than a
    /// window: the median climbs back through zero as soon as our leads outvote
    /// the stale negatives, unlike an EMA that could wind up for seconds.
    #[test]
    fn peer_led_history_does_not_delay_onset() {
        let mut t = Throttler::new();
        run(&mut t, -120, -5, 300);
        let n = frames_until(&mut t, 20, OPEN, 5.0);
        assert!(n <= WINDOW as u32, "onset deferred {n} frames after a peer-led history");
    }

    /// A sustained lead that still fits inside the present delay (negative
    /// speculation balance) never shaves fps, no matter the median lead — and
    /// once the balance crosses the boundary the brake comes on within a second.
    #[test]
    fn lead_inside_present_delay_is_free_until_the_boundary() {
        let mut t = Throttler::new();
        run(&mut t, 20, -2, 5_000);
        assert_eq!(t.step(20, -2), 0.0);
        let n = frames_until(&mut t, 20, 2, 5.0);
        assert!(n <= 60, "took {n} frames to engage past the boundary");
    }

    /// Regression: a long stretch of deep headroom must not defer engagement
    /// once the lead finally crosses the boundary. [`BALANCE_FLOOR`] bounds how
    /// far the balance EMA winds down, so the climb back through zero stays under
    /// a second.
    #[test]
    fn deep_headroom_does_not_delay_engagement() {
        let mut t = Throttler::new();
        run(&mut t, 20, OPEN, 5_000);
        run(&mut t, 20, -30, 1_000);
        let n = frames_until(&mut t, 20, 2, 5.0);
        assert!(n <= 60, "took {n} frames to engage past the boundary");
    }

    /// A balance fluttering across the boundary (±2, mean 0) neither throttles
    /// meaningfully nor flutters the fps target frame to frame — the balance EMA
    /// holds the gate near its mean.
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
}
