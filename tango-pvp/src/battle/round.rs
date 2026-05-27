use std::sync::Arc;

use parking_lot::Mutex as PlMutex;
use tokio::sync::Mutex;

use crate::input::{Input, Pair, PairQueue, PartialInput};

use super::throttler::Throttler;
use super::types::{BattleOutcome, CommittedState};
use super::EXPECTED_FPS;

/// EMA weight for the achieved-fps estimate (τ ≈ 20 frames ≈ 0.4 s at
/// 60 Hz) — fast enough to surface a sustained CPU deficit within half a
/// second, slow enough to ride out per-frame scheduler jitter.
const ACHIEVED_FPS_ALPHA: f32 = 0.05;
/// We advertise a reduced sustainable rate only once our achieved rate
/// sits this far below the target we're *trying* to hit. Above the
/// margin we report `EXPECTED_FPS` — a peer that's merely being throttled
/// by choice (meeting a lowered target) is not CPU-bound and must not
/// signal a constraint, or two peers would ratchet each other down.
const CPU_BOUND_MARGIN_FPS: f32 = 3.0;
/// Hard floor on the locally applied fps target, so a bogus or pathological
/// peer capacity can never stall the core outright.
const MIN_FPS_TARGET: f32 = 20.0;

/// Per-round state for the live primary. Owns the input queue, the
/// committed state, the Fastforwarder dedicated to this round, and the
/// helpers that wire remote-side prediction into FF runs.
pub struct Round {
    hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
    local_player_index: u8,
    current_tick: u32,
    /// Count of remote inputs received over the network this round. Equal
    /// to "highest remote tick + 1" since inputs arrive in order. Used as
    /// the receive-side half of the GGPO-style time-sync metric.
    last_remote_received_tick: u32,
    /// The remote's `current_tick - last_remote_received_tick` at their
    /// send time, copied from the most recently received network input.
    /// Stale by ~τ (one-way delay) but in steady state advantage is
    /// constant so staleness doesn't matter.
    last_remote_frame_advantage: i16,
    /// Active throttler strategy + its per-round state. Swappable at
    /// runtime via [`Round::set_throttler`].
    throttler: Box<dyn Throttler>,
    iq: PairQueue<PartialInput, PartialInput>,
    /// Joyflags + packet of the last committed remote input. Used as the
    /// seed for `hooks.predict_rx` when extending past the committed range.
    last_committed_remote_input: Input,
    committed_state: Option<CommittedState>,
    stepper: crate::stepper::Fastforwarder,
    replay_writer: Arc<PlMutex<Option<crate::replay::Writer>>>,
    primary_thread_handle: mgba::thread::Handle,
    sender: Arc<Mutex<Box<dyn crate::net::Sender + Send + Sync>>>,
    shadow: Arc<PlMutex<crate::shadow::Shadow>>,
    /// Live → display hand-off. Each frame the `present_state`
    /// (`frontier - frame_delay`) is published here for the display core.
    presentation: Arc<PlMutex<super::present::PresentationBuffer>>,
    /// Local presentation delay in frames. Read each frame to derive the
    /// display's target tick; settable live from the UI via [`super::Match`].
    frame_delay: Arc<std::sync::atomic::AtomicU32>,
    /// The single settled checkpoint the present roll advances. Lags
    /// the frontier by ~`frame_delay`; advances one tick per frame (it only
    /// moves forward, since the frontier does). `None` until the round's first
    /// commit seeds it.
    present_seed: Option<CommittedState>,
    /// Confirmed input pairs (real remote packet included) not yet consumed by
    /// the rolling seed: a sliding window `[confirmed_base, commit_frontier)`.
    /// Front entries are dropped as the seed rolls past them, so this stays
    /// small (~`frame_delay` deep) instead of growing for the whole round.
    confirmed: std::collections::VecDeque<Pair<Input, Input>>,
    /// Tick of `confirmed.front()` — lets us index the deque by absolute tick.
    confirmed_base: u32,
    /// Peer's most recently reported sustainable tick rate (fps). Caps our
    /// own fps target so we never outrun a CPU-bound peer. `EXPECTED_FPS`
    /// (the default) means "no constraint".
    peer_sustainable_fps: f32,
    /// EMA of our own achieved tick rate, measured from the wall-clock
    /// cadence of `add_local_input_and_fastforward` calls (one per emulated
    /// frame). Compared against `last_fps_target` to tell a CPU deficit
    /// apart from a deliberate throttle.
    achieved_fps_ema: f32,
    /// Timestamp of the previous frame, for the achieved-fps measurement.
    /// `None` until the round's first frame.
    last_frame_at: Option<std::time::Instant>,
    /// The fps target we last asked the core to hit. The gate that decides
    /// whether we're CPU-bound compares `achieved_fps_ema` against this, not
    /// against `EXPECTED_FPS`, so being capped low by a slow peer doesn't
    /// make us falsely report ourselves as the bottleneck.
    last_fps_target: f32,
    /// Whether the peer's reported capacity (not the skew throttler) is what
    /// last set our fps target. Surfaced as the `[P]` status-bar indicator.
    peer_cap_binding: bool,
}

impl Round {
    pub(super) fn new(match_: &super::Match, iq: PairQueue<PartialInput, PartialInput>) -> anyhow::Result<Self> {
        let hooks = match_.local_hooks();
        let stepper =
            crate::stepper::Fastforwarder::new(match_.rom(), hooks, match_.match_type(), match_.local_player_index())?;
        let last_committed_remote_input = Input {
            joyflags: 0,
            packet: vec![0u8; hooks.packet_size()],
        };
        Ok(Self {
            hooks,
            local_player_index: match_.local_player_index(),
            current_tick: 0,
            last_remote_received_tick: 0,
            last_remote_frame_advantage: 0,
            throttler: match_.build_throttler(),
            iq,
            last_committed_remote_input,
            committed_state: None,
            stepper,
            replay_writer: match_.replay_writer_handle(),
            primary_thread_handle: match_.primary_thread_handle(),
            sender: match_.sender_handle(),
            shadow: match_.shadow_handle(),
            presentation: match_.presentation_handle(),
            frame_delay: match_.frame_delay_handle(),
            present_seed: None,
            confirmed: std::collections::VecDeque::new(),
            confirmed_base: 0,
            peer_sustainable_fps: EXPECTED_FPS,
            achieved_fps_ema: EXPECTED_FPS,
            last_frame_at: None,
            last_fps_target: EXPECTED_FPS,
            peer_cap_binding: false,
        })
    }

    pub fn current_tick(&self) -> u32 {
        self.current_tick
    }

    pub fn increment_current_tick(&mut self) {
        self.current_tick += 1;
    }

    pub fn local_player_index(&self) -> u8 {
        self.local_player_index
    }

    pub fn set_first_committed_state(&mut self, local_state: Box<mgba::state::State>, first_packet: &[u8]) {
        let first = CommittedState {
            state: local_state,
            tick: 0,
            packet: first_packet.to_vec(),
        };
        // Seed the rolling present checkpoint at the round's tick-0 state.
        self.present_seed = Some(first.clone());
        self.confirmed_base = 0;
        self.committed_state = Some(first);
    }

    /// Called once per main_read_joyflags fire on the live primary. Sends
    /// the local input over the network, fills the queue, runs FF over
    /// committable input pairs, loads the dirty state into the live core,
    /// and writes any newly-committed inputs to the replay file.
    pub async fn add_local_input_and_fastforward(
        &mut self,
        mut core: mgba::core::CoreMutRef<'_>,
        joyflags: u16,
    ) -> anyhow::Result<Option<BattleOutcome>> {
        self.measure_achieved_fps();
        self.send_and_queue_local_input(joyflags).await?;

        let (input_pairs, last_committed_state, commit_tick, dirty_tick) = self.prepare_input_pairs();
        // Display target: the tick the display core renders, `frontier -
        // frame_delay`.
        let present_target = {
            let frame_delay = self.frame_delay.load(std::sync::atomic::Ordering::Relaxed);
            dirty_tick.saturating_sub(frame_delay)
        };
        let ff_result = self.run_fastforward(
            input_pairs,
            &last_committed_state,
            commit_tick,
            dirty_tick,
            present_target,
        )?;

        self.commit_remote_inputs(
            &ff_result.output_pairs,
            last_committed_state.tick,
            commit_tick,
            ff_result.round_result,
        );

        // The live core advances to the speculative frontier (it's the
        // netcode clock and must stay there); the display core renders the
        // delayed present_state we publish below.
        core.load_state(&ff_result.dirty_state.state).expect("load dirty state");
        self.committed_state = Some(ff_result.committed_state);

        let target = present_target;
        // Roll the settled checkpoint forward, clamped to the last
        // *committed* tick (commit_tick is the frontier of committed
        // inputs, exclusive — there's no confirmed input there yet).
        let rolled = self.roll_present_to(target.min(commit_tick.saturating_sub(1)))?;
        // In settled territory the rolled checkpoint IS the present; in the
        // rarer speculative range (frame_delay < rollback window) the live
        // FF captured the speculative present and the roll just keeps the
        // checkpoint current for when it settles again.
        let present_state = if target < commit_tick {
            rolled
        } else {
            ff_result.present_state.expect("live FF captures the speculative present")
        };
        self.presentation.lock().publish(present_target, present_state);
        self.update_fps_target(core);

        self.finalize_round(ff_result.round_result, commit_tick)
    }

    async fn send_and_queue_local_input(&mut self, joyflags: u16) -> anyhow::Result<()> {
        if !self.iq.can_add_local_input() {
            anyhow::bail!("local input buffer overflow!");
        }

        let frame_advantage = self.local_frame_advantage();
        self.sender
            .lock()
            .await
            .send(&crate::net::Input {
                joyflags,
                frame_advantage,
                sustainable_millifps: (self.local_sustainable_fps() * 1000.0).round() as u32,
            })
            .await?;

        self.add_local_input(PartialInput { joyflags });
        Ok(())
    }

    // Folds this frame's wall-clock interval into the achieved-fps EMA.
    // Called once per emulated frame; the interval includes any throttle
    // sleep, which is what we want — when we're meeting a lowered target the
    // EMA tracks that target, and local_sustainable_fps reads it as "not
    // CPU-bound".
    fn measure_achieved_fps(&mut self) {
        let now = std::time::Instant::now();
        if let Some(prev) = self.last_frame_at.replace(now) {
            let dt = now.duration_since(prev).as_secs_f32();
            if dt > 0.0 {
                let inst = 1.0 / dt;
                self.achieved_fps_ema += ACHIEVED_FPS_ALPHA * (inst - self.achieved_fps_ema);
            }
        }
    }

    // The capacity we advertise to the peer: EXPECTED_FPS unless we're
    // missing our own target by more than CPU_BOUND_MARGIN_FPS, in which case
    // we report the measured rate so the peer can pull back to let our
    // backlog drain.
    fn local_sustainable_fps(&self) -> f32 {
        if self.achieved_fps_ema < self.last_fps_target - CPU_BOUND_MARGIN_FPS {
            self.achieved_fps_ema
        } else {
            EXPECTED_FPS
        }
    }

    /// "How far ahead of the latest remote input I am." Sent in each
    /// outgoing packet so the peer can compute relative real-time skew.
    /// Saturating cast: clamps to i16 range, which fits any realistic
    /// frame advantage.
    pub fn local_frame_advantage(&self) -> i16 {
        let diff = self.current_tick as i32 - self.last_remote_received_tick as i32;
        diff.clamp(i16::MIN as i32, i16::MAX as i32) as i16
    }

    /// Peer's frame advantage as of their most recent packet — stale by
    /// ~τ (one-way delay) but matches what the throttler's skew estimate
    /// is reacting to.
    pub fn last_remote_frame_advantage(&self) -> i16 {
        self.last_remote_frame_advantage
    }

    /// True when our fps target is currently set by the peer's reported
    /// capacity rather than the skew throttler — i.e. we're holding back to
    /// let a CPU-bound peer keep up.
    pub fn peer_cap_binding(&self) -> bool {
        self.peer_cap_binding
    }

    fn prepare_input_pairs(&mut self) -> (Vec<Pair<PartialInput, PartialInput>>, CommittedState, u32, u32) {
        let (committable, predict_required) = self.iq.consume_and_peek_local();
        let last_committed_state = self.committed_state.take().expect("committed state");

        let commit_tick = last_committed_state.tick + committable.len() as u32;
        let dirty_tick = commit_tick + predict_required.len() as u32 - 1;

        let predicted_joyflags = predicted_remote_joyflags(self.last_committed_remote_input.joyflags);
        let input_pairs = committable
            .into_iter()
            .chain(predict_required.into_iter().map(|local| Pair {
                remote: PartialInput {
                    joyflags: predicted_joyflags,
                },
                local,
            }))
            .collect();

        (input_pairs, last_committed_state, commit_tick, dirty_tick)
    }

    fn run_fastforward(
        &mut self,
        input_pairs: Vec<Pair<PartialInput, PartialInput>>,
        last_committed_state: &CommittedState,
        commit_tick: u32,
        dirty_tick: u32,
        present_target: u32,
    ) -> anyhow::Result<crate::stepper::FastforwardResult> {
        let shadow = self.shadow.clone();
        let hooks = self.hooks;
        let mut last_commit = self.last_committed_remote_input.packet.clone();
        // Capture present_state here only for the speculative case
        // (`target >= commit_tick` — frame_delay below the rollback window):
        // those ticks aren't committed, so the rolling checkpoint can't reach
        // them and we use this live capture instead. Settled targets are
        // rolled from the confirmed window (see `roll_present_to`), so leave
        // present_tick past dirty_tick to capture nothing.
        let present_tick = if present_target >= commit_tick {
            present_target
        } else {
            u32::MAX
        };
        self.stepper.fastforward(
            &last_committed_state.state,
            input_pairs,
            last_committed_state.tick,
            commit_tick,
            dirty_tick,
            present_tick,
            &last_committed_state.packet,
            Box::new(move |tick, ip| {
                Ok(if tick < commit_tick {
                    let packet = shadow.lock().apply_input(tick, ip)?;
                    last_commit.clone_from(&packet);
                    packet
                } else {
                    hooks.predict_rx(&mut last_commit);
                    last_commit.clone()
                })
            }),
        )
    }

    /// Roll the single settled checkpoint forward to `target` (which is at or
    /// behind the commit frontier) and return the state there for the display.
    /// Replays the confirmed inputs from the seed's tick over the present
    /// stepper — no shadow (packets come from the confirmed window) and no
    /// prediction (every tick up to `target` is committed). The seed only ever
    /// moves forward; if `target` is at or behind it (frame_delay just
    /// increased, or no advance yet) we hold the current frame.
    fn roll_present_to(&mut self, target: u32) -> anyhow::Result<Box<mgba::state::State>> {
        let seed_tick = self.present_seed.as_ref().expect("present seed").tick;
        if target <= seed_tick {
            // Hold (don't roll backward): re-show the current checkpoint while
            // the frontier catches up to a freshly-increased frame_delay.
            return Ok(self.present_seed.as_ref().unwrap().state.clone());
        }

        let seed = self.present_seed.take().unwrap();
        let base = self.confirmed_base;
        // Inputs for ticks [seed_tick, target] — inclusive: the per-game
        // stepper trap peeks an input pair at the dirty tick before it captures
        // the dirty state, so the queue must still hold `target`'s input or the
        // capture is skipped and the FF spins forever. `target < commit_tick`
        // guarantees `confirmed` holds it.
        let input_pairs: Vec<Pair<PartialInput, PartialInput>> = (seed_tick..=target)
            .map(|t| {
                let pair = &self.confirmed[(t - base) as usize];
                Pair {
                    local: PartialInput {
                        joyflags: pair.local.joyflags,
                    },
                    remote: PartialInput {
                        joyflags: pair.remote.joyflags,
                    },
                }
            })
            .collect();
        let packets: Vec<Vec<u8>> = (seed_tick..=target)
            .map(|t| self.confirmed[(t - base) as usize].remote.packet.clone())
            .collect();

        // Reuse the live Fastforwarder (it's free at this point — the live run
        // already returned its result): the present roll covers a tick range
        // behind the commit frontier, disjoint from the live run, and each
        // `fastforward` call loads its own state fresh, so one core does both.
        let result = self.stepper.fastforward(
            &seed.state,
            input_pairs,
            seed_tick,
            target,
            target,
            u32::MAX, // no present capture in this run — we use its dirty state
            &seed.packet,
            Box::new(move |tick, _ip| Ok(packets[(tick - seed_tick) as usize].clone())),
        )?;
        // The FF's committed_state and dirty_state are both at `target`: keep
        // the committed one as the new rolling seed, hand the dirty one to the
        // display. Drop the now-consumed confirmed inputs ahead of the seed.
        self.present_seed = Some(result.committed_state);
        while self.confirmed_base < target {
            self.confirmed.pop_front();
            self.confirmed_base += 1;
        }
        Ok(result.dirty_state.state)
    }

    fn commit_remote_inputs(
        &mut self,
        output_pairs: &[Pair<Input, Input>],
        start_tick: u32,
        commit_tick: u32,
        round_result: Option<crate::stepper::RoundResult>,
    ) {
        for (i, ip) in output_pairs.iter().enumerate() {
            let tick = start_tick + i as u32;
            if tick >= commit_tick {
                break;
            }

            // Retain the confirmed pair (real remote packet) for the present
            // roll to replay. Commits run contiguously, so a push keeps the
            // deque's tail at `tick`; the front is pruned as the rolling seed
            // consumes it. Independent of the round-result replay cap below.
            debug_assert_eq!(self.confirmed_base + self.confirmed.len() as u32, tick);
            self.confirmed.push_back(ip.clone());

            if round_result.map_or(true, |rr| tick < rr.tick) {
                if let Some(writer) = self.replay_writer.lock().as_mut() {
                    // New replay format stores joyflags only; the per-tick
                    // packet is re-derived at playback time by running the
                    // shadow side from the recorded remote joyflags.
                    let partial_pair = Pair {
                        local: PartialInput {
                            joyflags: ip.local.joyflags,
                        },
                        remote: PartialInput {
                            joyflags: ip.remote.joyflags,
                        },
                    };
                    writer
                        .write_input(self.local_player_index, &partial_pair)
                        .expect("write input");
                }
            }
            self.last_committed_remote_input = ip.remote.clone();
        }
    }

    /// Swap the active throttler strategy. Mid-round-safe; the new
    /// strategy starts from its default state (no carry-over of the
    /// old one's smoothed skew / sustained counter / etc.).
    pub fn set_throttler(&mut self, throttler: Box<dyn Throttler>) {
        self.throttler = throttler;
    }

    fn update_fps_target(&mut self, mut core: mgba::core::CoreMutRef<'_>) {
        // Asymmetric time sync (only the leading peer slows; trailer
        // relies on the leader pulling back to converge). `local_adv`
        // and `last_remote_frame_advantage` both carry the symmetric
        // network-delay term τ; their difference isolates real-time
        // clock skew. `local_adv_A + local_adv_B` is a network-fixed
        // invariant (≈ 60·RTT), so the only reachable symmetric state is
        // local_adv_A = local_adv_B = sum/2 → raw_skew = 0, and the
        // asymmetric correction can't have both sides slowing
        // simultaneously in equilibrium.
        let local_advantage = self.local_frame_advantage() as i32;
        let remote_advantage = self.last_remote_frame_advantage as i32;
        let skew = local_advantage - remote_advantage;

        let slowdown = self.throttler.step(skew);
        let throttled = EXPECTED_FPS - slowdown;

        // Frame-skew alone tells the leader to slow only until the *frame*
        // gap closes; a CPU-bound trailer's deficit then hides as
        // ever-growing rollback depth instead. So additionally cap our
        // steady-state target at the peer's reported sustainable rate, so we
        // stop trying to balance against a 59.7 target it can never reach.
        // We cap at the peer's rate *exactly* (not below it) to avoid
        // inverting the imbalance and overflowing the peer's input queue;
        // the transient backlog that built up before the cap engaged drains
        // through `throttled`, which the skew throttler already pulls below
        // the cap whenever we're the leader. Only engages once the peer has
        // flagged itself CPU-bound; healthy peers report EXPECTED_FPS and
        // this is a no-op.
        let cap_engaged = self.peer_sustainable_fps < EXPECTED_FPS - CPU_BOUND_MARGIN_FPS;
        let fps_target = if cap_engaged {
            throttled.min(self.peer_sustainable_fps).max(MIN_FPS_TARGET)
        } else {
            throttled
        };
        // The peer cap "wins" only when it's engaged AND actually below what
        // the skew throttler alone would have allowed — otherwise the target
        // came from the throttler and the indicator stays off.
        self.peer_cap_binding = cap_engaged && self.peer_sustainable_fps <= throttled;

        self.last_fps_target = fps_target;
        core.gba_mut()
            .sync_mut()
            .expect("set fps target")
            .set_fps_target(fps_target);
    }

    fn finalize_round(
        &mut self,
        round_result: Option<crate::stepper::RoundResult>,
        commit_tick: u32,
    ) -> anyhow::Result<Option<BattleOutcome>> {
        let Some(round_result) = round_result else {
            return Ok(None);
        };
        if round_result.tick >= commit_tick {
            return Ok(None);
        }

        log::info!(
            "round finished at {:x} (real tick {:x})",
            round_result.tick,
            self.current_tick
        );

        Ok(Some(match round_result.outcome {
            crate::stepper::BattleOutcome::Draw => self.on_draw_outcome(),
            crate::stepper::BattleOutcome::Loss => BattleOutcome::Loss,
            crate::stepper::BattleOutcome::Win => BattleOutcome::Win,
        }))
    }

    pub fn on_draw_outcome(&self) -> BattleOutcome {
        match self.local_player_index {
            0 => BattleOutcome::Win,
            1 => BattleOutcome::Loss,
            _ => unreachable!(),
        }
    }

    pub fn has_committed_state(&mut self) -> bool {
        self.committed_state.is_some()
    }

    pub fn add_local_input(&mut self, input: PartialInput) {
        log::debug!("local input: {:?}", input);
        self.iq.add_local_input(input);
    }

    pub fn add_remote_input(&mut self, input: crate::net::Input) {
        log::debug!("remote input: {:?}", input);
        self.iq.add_remote_input(PartialInput {
            joyflags: input.joyflags,
        });
        self.last_remote_received_tick = self.last_remote_received_tick.wrapping_add(1);
        self.last_remote_frame_advantage = input.frame_advantage;
        // Wire value is milli-fps; back to fps and clamp into the sane range,
        // so a peer that under- or over-claims (incl. the "keeping up" value
        // that exceeds native) can't push our target below the floor or above
        // native. A value below EXPECTED_FPS means it's asking us to slow to
        // its sustainable rate.
        self.peer_sustainable_fps = (input.sustainable_millifps as f32 / 1000.0).clamp(MIN_FPS_TARGET, EXPECTED_FPS);
    }

    pub(super) fn can_add_remote_input(&self) -> bool {
        self.iq.can_add_remote_input()
    }
}

impl Drop for Round {
    fn drop(&mut self) {
        // HACK: This is the only safe way to set the FPS without clogging everything else up.
        self.primary_thread_handle
            .lock_audio()
            .sync_mut()
            .set_fps_target(EXPECTED_FPS);
    }
}

fn predicted_remote_joyflags(last_remote_joyflags: u16) -> u16 {
    const HELD_KEYS: u16 = mgba::input::keys::A as u16 | mgba::input::keys::B as u16;
    last_remote_joyflags & HELD_KEYS
}
