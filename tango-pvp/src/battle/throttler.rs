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

/// EMA weight applied while skew is growing. τ ≈ 5 s rise (at 60 Hz).
const ALPHA_SLOWDOWN: f32 = 1.0 / 300.0;
/// EMA weight applied while skew is shrinking. τ ≈ 0.5 s fall (at 60 Hz).
const ALPHA_SPEEDUP: f32 = 1.0 / 30.0;
/// EMA weight for the speculation balance. τ ≈ 0.5 s (at 60 Hz): the raw
/// balance moves in bursts as remote inputs arrive, and smoothing it keeps
/// that flutter out of the emitted fps target.
const ALPHA_BALANCE: f32 = 1.0 / 30.0;
/// Gain from smoothed skew to emitted slowdown, in fps shaved per tick of
/// skew. At 1.0 the throttler is a proportional controller whose correction
/// rate equals its error: a smoothed skew of S sheds lead at ~S ticks/s,
/// converging exponentially with a ~1 s time constant once engaged.
const SKEW_TO_SLOWDOWN: f32 = 1.0;
/// Engagement ramp at the speculation boundary: fps of slowdown permitted per
/// tick of smoothed speculative depth. Deliberately steep — the ramp decides
/// *when* throttling engages (at the boundary, continuously), not how hard;
/// magnitude is still the smoothed skew's job, which takes over within the
/// first few ticks of depth.
const ENGAGEMENT_SLOPE: f32 = 10.0;
/// Slowdown ceiling, in fps below the base rate.
const MAX_SLOWDOWN: f32 = 30.0;
/// Floor on the smoothed speculation balance. Deep headroom (lead pinned well
/// inside the present delay) would otherwise wind the balance EMA down toward
/// -present_delay, and the climb back through zero would defer engagement past
/// the boundary by however long the slider made it — the skew windup all over
/// again, scaled by a user setting. The ramp saturates the slowdown ceiling at
/// MAX_SLOWDOWN/ENGAGEMENT_SLOPE ticks of depth, so flooring at its mirror
/// image keeps the gate's swing symmetric around the boundary and the
/// re-engagement latency constant, while leaving enough negative range to
/// absorb bursty-arrival flutter.
const BALANCE_FLOOR: f32 = -(MAX_SLOWDOWN / ENGAGEMENT_SLOPE);

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
    /// value keeps that flutter out of the fps target. Floored at
    /// [`BALANCE_FLOOR`]: deep headroom would otherwise wind it down toward
    /// -present_delay, and the climb back through zero would hold the brake
    /// off well past the boundary crossing.
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
    /// `local_advantage - remote_advantage`; its asymmetric EMA, scaled by
    /// [`SKEW_TO_SLOWDOWN`], sets the slowdown magnitude. `speculation_balance`
    /// is the engine's signed
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
        self.smoothed_balance = (ALPHA_BALANCE * speculation_balance as f32
            + (1.0 - ALPHA_BALANCE) * self.smoothed_balance)
            .max(BALANCE_FLOOR);
        (SKEW_TO_SLOWDOWN * self.smoothed)
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

    /// Regression: a long stretch of deep headroom (lead pinned far inside a
    /// large present delay) must not defer engagement once the lead finally
    /// crosses the boundary. Without the floor the balance EMA winds down
    /// toward -30 here and the climb back through zero holds the brake off
    /// for ~1.7 s instead of under a second.
    #[test]
    fn deep_headroom_does_not_delay_engagement() {
        let mut t = Throttler::new();
        // Saturate the skew EMA so the balance gate is what binds below.
        run(&mut t, 20, OPEN, 5_000);
        run(&mut t, 20, -30, 1_000);
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

    /// Per-frame skew each peer's throttler saw over a closed-loop run.
    struct SimOut {
        skew_p1: Vec<i32>,
        skew_p2: Vec<i32>,
    }

    /// Closed-loop simulation of the live in-match clock-sync between two peers
    /// over a datagram channel with `loss_pct`% loss and `latency` frames of
    /// one-way delay. Each peer, every frame:
    ///
    /// * produces an input at its current (throttled) tick rate,
    /// * ships its frontier + reported advantage to the other, dropping
    ///   `loss_pct`% of datagrams (survivors arrive `latency` frames later),
    /// * carries a redundancy window so a drop is recovered by a later frame and
    ///   the remote stream stays contiguous (as `OutStream`/`InStream` do), and
    /// * runs the real [`Throttler`] on its skew, whose slowdown feeds back into
    ///   how fast it produces the next inputs.
    ///
    /// Only the leading peer brakes, so the two hand the lead back and forth.
    /// With a full round trip of dead-time in that feedback and packet loss
    /// perturbing the skew, the handoff overshoots into the large, plateauing
    /// oscillation the live telemetry shows at 100 ms / 10% loss. The test pins
    /// that (bad) amplitude so a throttler/transport change can be tuned against it.
    fn simulate(loss_pct: u64, latency: u32, frames: u32, predict: bool) -> SimOut {
        const REDUNDANCY: u32 = 64;
        const PRESENT_DELAY: i32 = 2;
        const BASE_FPS: f32 = 60.0;

        struct Peer {
            frontier: u32,                            // inputs produced (own tick)
            credit: f32,                              // fractional-tick accumulator
            fps: f32,                                 // current (throttled) production rate
            ahead: std::collections::BTreeSet<u32>,   // remote ticks past `contiguous`
            contiguous: u32,                          // highest contiguous remote tick
            last_advance: u32,                        // frame `contiguous` last moved
            last_remote_adv: i32,                     // freshest advantage the remote reported
            throttler: Throttler,
        }
        impl Peer {
            fn new() -> Self {
                Peer {
                    frontier: 0,
                    credit: 0.0,
                    fps: BASE_FPS,
                    ahead: std::collections::BTreeSet::new(),
                    contiguous: 0,
                    last_advance: 0,
                    last_remote_adv: 0,
                    throttler: Throttler::new(),
                }
            }
            /// Raw lead: how far local input outruns the *delivered* remote
            /// frontier. Drives speculation — it really does ramp during a stall.
            fn lead(&self) -> i32 {
                self.frontier as i32 - self.contiguous as i32
            }
            /// Clock-sync lead with the remote frontier extrapolated through a
            /// delivery stall. A lost burst freezes `contiguous`, but the remote
            /// clock keeps running, so credit it one tick per frame since it last
            /// advanced. That's what stops the lead — and the skew — ramping while
            /// we're simply not hearing from the peer.
            fn synced_lead(&self, frame: u32) -> i32 {
                let stall = frame - self.last_advance;
                self.frontier as i32 - (self.contiguous + stall) as i32
            }
            fn deliver(&mut self, frame: u32, frontier: u32, adv: i32) {
                let before = self.contiguous;
                let lo = frontier.saturating_sub(REDUNDANCY - 1).max(1);
                for t in lo..=frontier {
                    if t > self.contiguous {
                        self.ahead.insert(t);
                    }
                }
                while self.ahead.remove(&(self.contiguous + 1)) {
                    self.contiguous += 1;
                }
                if self.contiguous != before {
                    self.last_advance = frame;
                }
                self.last_remote_adv = adv;
            }
        }

        let mut peers = [Peer::new(), Peer::new()];
        // In-flight datagrams: (arrival frame, recipient, sender frontier, advantage).
        let mut wire: Vec<(u32, usize, u32, i32)> = Vec::new();
        // Loss is *bursty* — real packet loss is correlated (router-queue
        // overflow drops runs), and the lossy transport's shed/redundancy
        // recovery clumps it further. A burst drops ~BURST consecutive datagrams
        // in one direction, so the receiver's contiguous frontier stalls that
        // long and the skew ramps then snaps back — independent loss recovers in
        // one frame and barely moves it. Deterministic LCG, so it's reproducible.
        const BURST: u64 = 12;
        let start_per_million = if loss_pct == 0 {
            0
        } else {
            loss_pct * 1_000_000 / ((100 - loss_pct) * BURST)
        };
        let mut rng = 0x1234_5678_9abc_def0u64;
        let mut bad = [0u64; 2]; // frames left in a loss burst, per direction
        let mut next = || {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            rng >> 40
        };

        let mut out = SimOut {
            skew_p1: Vec::with_capacity(frames as usize),
            skew_p2: Vec::with_capacity(frames as usize),
        };

        for f in 0..frames {
            // Produce this frame's input, gated by each peer's throttled rate.
            for p in &mut peers {
                p.credit += p.fps / BASE_FPS;
                if p.credit >= 1.0 {
                    p.frontier += 1;
                    p.credit -= 1.0;
                }
            }
            // Ship each peer's frontier + reported advantage, dropping loss bursts.
            for src in 0..2 {
                let lost = if bad[src] > 0 {
                    bad[src] -= 1;
                    true
                } else if next() % 1_000_000 < start_per_million {
                    bad[src] = BURST - 1; // this frame, plus BURST-1 more
                    true
                } else {
                    false
                };
                if !lost {
                    // The advantage shipped is the local clock-sync lead: the
                    // raw lead today, the stall-extrapolated lead with the fix.
                    let adv = if predict {
                        peers[src].synced_lead(f)
                    } else {
                        peers[src].lead()
                    };
                    let frontier = peers[src].frontier;
                    wire.push((f + latency, 1 - src, frontier, adv));
                }
            }
            // Deliver everything scheduled to land this frame.
            let mut i = 0;
            while i < wire.len() {
                if wire[i].0 == f {
                    let (_, dst, frontier, adv) = wire[i];
                    peers[dst].deliver(f, frontier, adv);
                    wire.swap_remove(i);
                } else {
                    i += 1;
                }
            }
            // Each peer reads its skew, throttles, and sets next frame's rate.
            // Clock sync uses the extrapolated lead with the fix, the raw lead
            // without it; speculation always gates on the raw lead.
            for i in 0..2 {
                let local_adv = if predict {
                    peers[i].synced_lead(f)
                } else {
                    peers[i].lead()
                };
                let skew = local_adv - peers[i].last_remote_adv;
                let balance = peers[i].lead().max(0) - PRESENT_DELAY;
                let slowdown = peers[i].throttler.step(skew, balance);
                peers[i].fps = BASE_FPS - slowdown;
                if i == 0 {
                    out.skew_p1.push(skew);
                } else {
                    out.skew_p2.push(skew);
                }
            }
        }
        out
    }

    fn range(series: &[i32]) -> (i32, i32) {
        (*series.iter().min().unwrap(), *series.iter().max().unwrap())
    }

    /// The live telemetry's skew sawtooth at 100 ms / 10% loss, and the fix.
    ///
    /// A loss burst stalls the contiguous frontier, so the lead ramps then snaps
    /// back over tens of ticks — the wide sawtooth in the telemetry. But a stall
    /// doesn't move the remote clock, it just withholds its inputs, so that ramp
    /// is a measurement artifact, not real divergence. Extrapolating the remote
    /// frontier through the stall ([`Peer::synced_lead`]) removes it: the skew
    /// clock sync + the display act on stays tight while the raw lead (still used
    /// for speculation) heaves. Pins both so the fix can't regress.
    #[test]
    fn extrapolation_tames_skew_under_packet_loss() {
        let frames = 6_000; // 100 s at 60 Hz
        let latency = 6; // ~100 ms one way
        let warmup = 1_200; // let the loop reach steady state before measuring
        let span = |(lo, hi): (i32, i32)| hi - lo;
        let lossy_span = |predict| span(range(&simulate(10, latency, frames, predict).skew_p1[warmup..]));

        let before = lossy_span(false); // raw lead → the telemetry sawtooth
        let after = lossy_span(true); // extrapolated through the stall
        let clean = span(range(&simulate(0, latency, frames, true).skew_p1[warmup..]));
        eprintln!("P1 skew span @ 100ms/10% loss — raw lead {before}, extrapolated {after}; clean link {clean}");

        // Today (raw lead): a wide sawtooth, the bad behaviour from the screenshot.
        assert!(before >= 20, "raw-lead skew should reproduce the wide swing, got {before}");
        // Extrapolated: clock sync + the display see a tight, near-flat skew...
        assert!(after <= 4, "extrapolated skew should stay tight under loss, got {after}");
        // ...a small fraction of the raw swing, and no worse than a clean link.
        assert!(after * 4 <= before, "extrapolated {after} should be far under raw {before}");
        assert!(clean <= 2, "clean-link skew should settle, got {clean}");
    }
}
