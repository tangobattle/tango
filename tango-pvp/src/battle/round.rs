use std::sync::Arc;

use parking_lot::Mutex as PlMutex;
use tokio::sync::Mutex;

use crate::input::{Input, Pair, PairQueue, PartialInput};

use super::throttler::Throttler;
use super::types::{BattleOutcome, CommittedState};
use super::EXPECTED_FPS;

/// Per-round state for the live primary. Owns the input queue, the settled
/// checkpoint (`settled_state`), the Fastforwarder dedicated to this round,
/// and the helpers that wire remote-side prediction into FF runs.
pub struct Round {
    // ---- Per-round constants (set at construction) ----
    hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
    local_player_index: u8,
    /// Shared input delay in frames (`min` of the two peers' `frame_delay`).
    /// Local input is delay-lined by this much before it's queued, and the
    /// remote queue is seeded with this many neutral inputs at round start, so
    /// the committed frontier sits `input_delay` closer to the live frontier —
    /// i.e. rollback depth drops by `input_delay`. Symmetric, so it's "fair".
    input_delay: u32,
    /// Local presentation delay in frames (`frontier − presentation_delay` is
    /// the display's target tick). Fixed for the match; the netcode-affecting
    /// part of this side's requested `frame_delay` lives in `input_delay`.
    presentation_delay: u32,

    // ---- Emulator + I/O handles ----
    /// Headless FF emulator. Used by the settle FF (to advance the settled
    /// checkpoint over committed inputs) and the speculative-tail FF (to
    /// render the display state when the target is past the commit frontier).
    stepper: crate::stepper::Fastforwarder,
    /// Local shadow emulator — simulates the opponent's side from the
    /// joyflags we both put on the wire.
    shadow: Arc<PlMutex<crate::shadow::Shadow>>,
    /// Handle to the live core's mgba thread. Held so the Round's `Drop` can
    /// reset its `fps_target` when the round ends.
    primary_thread_handle: mgba::thread::Handle,
    /// Outbound network input channel.
    sender: Arc<Mutex<Box<dyn crate::net::Sender + Send + Sync>>>,
    /// Replay sink (None when not recording).
    replay_writer: Arc<PlMutex<Option<crate::replay::Writer>>>,

    // ---- Tick tracking ----
    /// Netcode frontier: advances one per wall-frame via the post-tick hook
    /// on the live core. Decoupled from the game's tick (which lags by
    /// `presentation_delay` once the round's warmup elapses) — internal use
    /// only, per-game traps that need a tick should reach for
    /// [`Self::last_loaded_tick`] instead.
    frontier: u32,
    /// Tick of the last `present_state` loaded into the live core (0 before
    /// any load — same as the game's initial tick, so callers don't need a
    /// special pre-load case). The live core's game tick advances naturally
    /// from here, so per-game `primary` traps that need the game's tick read
    /// [`Self::last_loaded_tick`] and add `+ 1` once
    /// `round_post_increment_tick` has fired. Decoupled from [`Self::frontier`]
    /// (the netcode frontier), which advances at wall-clock rate via the
    /// post-tick hook regardless of how far the live core lags.
    last_loaded_tick: u32,

    // ---- Input pipeline ----
    /// Depth-`input_delay` ring delaying the local joyflags before they enter
    /// the queue: the input committed at tick T is the one sampled at T −
    /// `input_delay` (neutral for the first `input_delay` ticks). Pre-seeded
    /// with `input_delay` neutral frames. The value *sent on the wire* is the
    /// raw, un-delayed sample — the peer's matching remote-queue prefill is what
    /// realizes the delay on their side.
    local_input_delay_line: std::collections::VecDeque<u16>,
    /// Paired local/remote input queue. Front entries are drained each frame
    /// (up to `min(local.len(), remote.len())`) into `pending_commits`; the
    /// remaining local entries are the speculative window.
    input_queue: PairQueue<PartialInput, PartialInput>,

    // ---- Commit + settled-state checkpoint ----
    /// Exclusive upper bound of ticks where both peers' real inputs are known
    /// (= `settled_state.tick + pending_commits.len()`). Advances by
    /// `committable.len()` each frame as `input_queue` drains paired inputs.
    commit_frontier: u32,
    /// Joyflags + packet of the most recent input the settle FF actually
    /// processed. Used as the seed for `hooks.predict_rx` in the speculative
    /// tail. Updated from the settle's `FastforwardResult.output_pairs`, so
    /// it sits at `settled_state.tick − 1` after each settle — one tick
    /// behind `commit_frontier − 1` in the speculative regime (acceptable;
    /// the tail is throwaway).
    last_committed_remote_input: Input,
    /// Joyflags pairs for ticks `[settled_state.tick, commit_frontier)`,
    /// awaiting consumption by the settle. New commits push to the back as
    /// `input_queue` drains; the settle pops the front as it advances.
    /// Packets aren't stored — the settle derives the real ones on demand
    /// via `shadow.apply_input`, paced at the display rate instead of the
    /// wire rate.
    pending_commits: std::collections::VecDeque<Pair<PartialInput, PartialInput>>,
    /// The single settled checkpoint that drives both the display and the
    /// committed-side bookkeeping (shadow, replay, round-end detection). In
    /// the settled regime its captured state IS what the live core renders;
    /// in the speculative regime a throwaway tail FF starts from it. Capped
    /// at `commit_frontier − 1`, so the seed only ever holds real, committed
    /// state.
    settled_state: Option<CommittedState>,

    // ---- Time sync / throttling ----
    /// Count of remote inputs received over the network this round. Equal
    /// to "highest remote tick + 1" since inputs arrive in order. Used as
    /// the receive-side half of the GGPO-style time-sync metric.
    last_remote_received_tick: u32,
    /// The remote's `frontier - last_remote_received_tick` at their send
    /// time, copied from the most recently received network input. Stale
    /// by ~τ (one-way delay) but in steady state advantage is constant so
    /// staleness doesn't matter.
    last_remote_frame_advantage: i16,
    /// Throttler + its per-round state. Hardcoded to the asymmetric EMA
    /// (see [`super::throttler`]); fresh instance per round.
    throttler: Box<dyn Throttler>,
}

impl Round {
    pub(super) fn new(
        match_: &super::Match,
        mut input_queue: PairQueue<PartialInput, PartialInput>,
    ) -> anyhow::Result<Self> {
        let hooks = match_.local_hooks();
        let stepper =
            crate::stepper::Fastforwarder::new(match_.rom(), hooks, match_.match_type(), match_.local_player_index())?;
        let last_committed_remote_input = Input {
            joyflags: 0,
            packet: vec![0u8; hooks.packet_size()],
        };

        let input_delay = match_.input_delay();
        // Seed the remote queue with `input_delay` neutral inputs: this is the
        // peer's "head start". Their inputs are relabeled `input_delay` ticks
        // ahead (the early ticks are free neutrals), so the committed frontier
        // reaches `input_delay` further than one-per-frame arrivals alone would
        // — that's the rollback reduction. These prefilled neutrals must NOT
        // bump `last_remote_received_tick` (the throttler's metric tracks the
        // *real* network relationship), so push them straight onto the queue
        // rather than through `add_remote_input`.
        for _ in 0..input_delay {
            input_queue.add_remote_input(PartialInput { joyflags: 0 });
        }

        Ok(Self {
            // constants
            hooks,
            local_player_index: match_.local_player_index(),
            input_delay,
            presentation_delay: match_.presentation_delay(),
            // emulator + I/O handles
            stepper,
            shadow: match_.shadow_handle(),
            primary_thread_handle: match_.primary_thread_handle(),
            sender: match_.sender_handle(),
            replay_writer: match_.replay_writer_handle(),
            // tick tracking
            frontier: 0,
            last_loaded_tick: 0,
            // input pipeline — local delay line pre-seeded with `input_delay`
            // neutral frames so the first `input_delay` committed local inputs
            // are neutral, mirroring the remote prefill above.
            local_input_delay_line: std::collections::VecDeque::from(vec![0u16; input_delay as usize]),
            input_queue,
            // commit + settled-state checkpoint
            commit_frontier: 0,
            last_committed_remote_input,
            pending_commits: std::collections::VecDeque::new(),
            settled_state: None,
            // time sync / throttling
            last_remote_received_tick: 0,
            last_remote_frame_advantage: 0,
            throttler: match_.build_throttler(),
        })
    }

    /// Netcode frontier — advances one per wall-frame via the post-tick hook,
    /// regardless of how far the live core lags. Internal to the crate; per-game
    /// traps that need a tick should reach for [`Self::last_loaded_tick`]
    /// (the game's tick) instead.
    pub(crate) fn frontier(&self) -> u32 {
        self.frontier
    }

    /// Tick of the last `present_state` we loaded into the live core, or `0`
    /// before any load. The live core's game tick is this value while in-tick
    /// processing (between `main_read_joyflags` and
    /// `round_post_increment_tick`), and this value `+ 1` once
    /// `round_post_increment_tick` has fired (i.e. at the next
    /// `main_read_joyflags`). Decoupled from `frontier` (the netcode frontier,
    /// which runs `presentation_delay` ticks ahead). Per-game primary traps
    /// that need the game's tick (not the netcode frontier) read this and
    /// apply the appropriate `+ 1` for the phase they fire in.
    pub fn last_loaded_tick(&self) -> u32 {
        self.last_loaded_tick
    }

    /// Called from each per-game `round_post_increment_tick` trap (wired once
    /// per game tick on the live core) to keep the frontier in lockstep with
    /// the wall clock. Not "increment the current tick" in the game sense —
    /// the game's tick is [`Self::last_loaded_tick`] (+ phase offset).
    pub fn advance_frontier(&mut self) {
        self.frontier += 1;
    }

    pub fn local_player_index(&self) -> u8 {
        self.local_player_index
    }

    pub fn set_first_settled_state(&mut self, local_state: Box<mgba::state::State>, first_packet: &[u8]) {
        // Seed the settled checkpoint at the round's tick-0 state. `commit_frontier`
        // starts at 0 and `pending_commits` is empty; the chain grows from here
        // as `add_local_input_and_fastforward` drains the input_queue.
        self.settled_state = Some(CommittedState {
            state: local_state,
            tick: 0,
            packet: first_packet.to_vec(),
        });
    }

    /// Called once per main_read_joyflags fire on the live primary. Sends
    /// the local input over the network, fills the queue, settles the
    /// checkpoint forward, runs a speculative tail when the display target
    /// is past the commit frontier, and loads the displayed state into the
    /// live core. The settle is the single FF chain — shadow advancement,
    /// real-packet derivation, replay write, and round-end detection all
    /// hang off it, paced at the display rate.
    pub async fn add_local_input_and_fastforward(
        &mut self,
        mut core: mgba::core::CoreMutRef<'_>,
        joyflags: u16,
    ) -> anyhow::Result<Option<BattleOutcome>> {
        self.send_and_queue_local_input(joyflags).await?;

        // Drain the queue. Newly-paired joyflags pairs go straight onto
        // `pending_commits` as joyflags-only — packets are derived on demand
        // by the settle below, not produced eagerly here.
        let (committable, peeked) = self.input_queue.consume_and_peek_local();
        self.commit_frontier += committable.len() as u32;
        self.pending_commits.extend(committable);

        // Display target. `frontier` is the netcode frontier (advances 1/wall-frame
        // via the post-tick hook regardless of where the live core is loaded), so
        // `frontier - presentation_delay` is what the user sees.
        let target = self.frontier.saturating_sub(self.presentation_delay);

        // Settle the checkpoint forward, *capped* at the last committed tick
        // (`commit_frontier - 1`). The seed must never absorb predicted
        // packets — when commit_frontier later catches up, the real packets at
        // those ticks differ from what predict_rx produced, leaving the seed
        // stale and snowballing the error every subsequent frame.
        let settled_target = target.min(self.commit_frontier.saturating_sub(1));
        let round_result = self.settle_to(settled_target)?;

        let present_state = if target > settled_target && self.commit_frontier > 0 {
            // Speculative tail: throwaway FF from settled_state to target,
            // predicting every packet in `[settled_state.tick, target)`
            // (the leading tick at `commit_frontier − 1` is committed
            // joyflags-wise but uses a predicted packet too, since the
            // shadow can't advance through it twice — the next settle will
            // re-process it with the real packet).
            self.speculate_tail(target, &peeked)?
        } else {
            // Either settled (`target ≤ commit_frontier − 1`) — settled_state
            // IS the display — or pre-first-commit, where there's no real
            // packet to seed predict_rx from anyway, so we just hold the
            // initial state.
            self.settled_state.as_ref().unwrap().state.clone()
        };

        core.load_state(&present_state).expect("load present state");
        self.last_loaded_tick = target;
        self.update_fps_target(core);

        self.finalize_round(round_result, self.commit_frontier)
    }

    /// Throwaway FF from `settled_state` to `target`, predicting all remote
    /// packets in the range. Used only when `target > commit_frontier − 1`
    /// (rollback depth exceeds presentation delay). Doesn't touch
    /// `settled_state` or the shadow — the next frame's settle re-processes
    /// the committed portion with real packets.
    fn speculate_tail(&mut self, target: u32, peeked: &[PartialInput]) -> anyhow::Result<Box<mgba::state::State>> {
        let seed = self.settled_state.as_ref().expect("settled state");
        let seed_tick = seed.tick;
        debug_assert_eq!(
            seed_tick,
            self.commit_frontier.saturating_sub(1),
            "speculative tail seed must sit at the settled cap"
        );

        let predicted_joyflags = predicted_remote_joyflags(self.last_committed_remote_input.joyflags);
        let total = (target - seed_tick + 1) as usize;
        let mut input_pairs: Vec<Pair<PartialInput, PartialInput>> = Vec::with_capacity(total);

        // First entry sits at the committed cap (`commit_frontier − 1`): real
        // joyflags from the pending-commits front (which the settle
        // deliberately left there), paired with a predicted packet below.
        input_pairs.push(self.pending_commits[0].clone());
        // Trailing entries are pure speculation: local joyflags from `peeked`
        // (the unconsumed tail of the input_queue) paired with held
        // remote-joyflags prediction. We need `total − 1 = target −
        // (commit_frontier − 1)` peeked entries (= `rollback_depth − PD + 1`);
        // the slice panics if `peeked` is short, matching the original.
        for local in &peeked[..total - 1] {
            input_pairs.push(Pair {
                local: local.clone(),
                remote: PartialInput {
                    joyflags: predicted_joyflags,
                },
            });
        }

        // Pre-advance the predict seed once so on-entry `predict_packet`
        // already represents the *current* tick's packet (returns-then-predicts).
        // `last_committed_remote_input` sits at `seed_tick − 1` here, so the
        // pre-advance moves the seed forward to represent `seed_tick`.
        let hooks = self.hooks;
        let mut predict_packet = self.last_committed_remote_input.packet.clone();
        hooks.predict_rx(&mut predict_packet);
        let result = self.stepper.fastforward(
            &seed.state,
            input_pairs,
            seed_tick,
            target,
            &seed.packet,
            Box::new(move |_tick, _ip| {
                let out = predict_packet.clone();
                hooks.predict_rx(&mut predict_packet);
                Ok(out)
            }),
        )?;
        Ok(result.state.state)
    }

    async fn send_and_queue_local_input(&mut self, joyflags: u16) -> anyhow::Result<()> {
        if !self.input_queue.can_add_local_input() {
            anyhow::bail!("local input buffer overflow!");
        }

        let frame_advantage = self.local_frame_advantage();
        // Send the raw sample. The peer realizes the input delay on its end via
        // its remote-queue prefill, so the wire carries un-delayed joyflags.
        self.sender
            .lock()
            .await
            .send(&crate::net::Input {
                joyflags,
                frame_advantage,
            })
            .await?;

        // Queue the delay-lined sample: what we commit at this tick is the
        // joyflags from `input_delay` frames ago (neutral for the first
        // `input_delay` ticks). One value in, one out — the ring shifts content
        // without changing the queue's per-frame growth.
        self.local_input_delay_line.push_back(joyflags);
        let delayed = self.local_input_delay_line.pop_front().unwrap_or(0);
        self.add_local_input(PartialInput { joyflags: delayed });
        Ok(())
    }

    /// "How far ahead of the latest remote input I am." Sent in each
    /// outgoing packet so the peer can compute relative real-time skew.
    /// Saturating cast: clamps to i16 range, which fits any realistic
    /// frame advantage.
    pub fn local_frame_advantage(&self) -> i16 {
        let diff = self.frontier as i32 - self.last_remote_received_tick as i32;
        diff.clamp(i16::MIN as i32, i16::MAX as i32) as i16
    }

    /// Peer's frame advantage as of their most recent packet — stale by
    /// ~τ (one-way delay) but matches what the throttler's skew estimate
    /// is reacting to.
    pub fn last_remote_frame_advantage(&self) -> i16 {
        self.last_remote_frame_advantage
    }

    /// Speculative depth — local inputs queued past the latest remote, i.e.
    /// the number of frames a real remote packet can force us to roll back
    /// and re-simulate. Surfaced in the status bar as "depth".
    pub fn rollback_depth(&self) -> u32 {
        self.input_queue.speculative_depth() as u32
    }

    /// Shared input delay applied this match (`min` of the two peers'
    /// frame_delay). Constant for the match; this much was already shaved off
    /// `rollback_depth` versus a pure-presentation-delay setup.
    pub fn input_delay(&self) -> u32 {
        self.input_delay
    }

    /// Settle the checkpoint forward to `target` (which is at or behind the
    /// commit frontier). Single FF over `pending_commits` from the seed: the
    /// callback drives `shadow.apply_input` to derive real remote packets,
    /// the stepper detects round end, and the captured state becomes both the
    /// next seed and (in the settled regime) the display load. Pops the
    /// consumed front entries off `pending_commits`, updates
    /// `last_committed_remote_input` from the FF's output, and writes the
    /// just-committed joyflags to the replay file. Holds (no-op) when `target`
    /// is at or behind the seed.
    fn settle_to(&mut self, target: u32) -> anyhow::Result<Option<crate::stepper::RoundResult>> {
        let seed_tick = self.settled_state.as_ref().expect("settled state").tick;
        if target <= seed_tick {
            // Hold: re-show the current checkpoint while the frontier climbs
            // to `presentation_delay` ticks past the round's start (the
            // present can't move until then).
            return Ok(None);
        }

        let seed = self.settled_state.take().expect("settled state");
        // Inputs for ticks `[seed_tick, target]` — inclusive: the per-game
        // stepper trap peeks an input pair at `capture_tick` before it
        // snapshots, so `pending_commits` must still hold `target`'s entry or
        // the capture is skipped and the FF spins forever. `target ≤
        // commit_frontier − 1` guarantees `pending_commits` is long enough.
        let count = (target - seed_tick + 1) as usize;
        debug_assert!(self.pending_commits.len() >= count);
        let input_pairs: Vec<Pair<PartialInput, PartialInput>> =
            self.pending_commits.iter().take(count).cloned().collect();

        let shadow = self.shadow.clone();
        let result = self.stepper.fastforward(
            &seed.state,
            input_pairs,
            seed_tick,
            target,
            &seed.packet,
            Box::new(move |tick, ip| shadow.lock().apply_input(tick, ip)),
        )?;

        // The FF's `output_pairs` cover ticks `[seed_tick, target − 1]` —
        // the placeholder at `target` is peeked but not applied, so it
        // doesn't produce an output. The last entry is the most recent
        // committed input the shadow processed; promote its remote side to
        // `last_committed_remote_input` for the next speculative tail's
        // predict_rx seed.
        if let Some(last_pair) = result.output_pairs.last() {
            self.last_committed_remote_input = last_pair.remote.clone();
        }

        // Mirror the old commit_remote_inputs replay path, now paced at the
        // display rate. The round_result gate drops the few ticks the FF
        // simulated past the round-ending tick (the stepper exits at the
        // capture point, not at the round-ending tick). Joyflags only — the
        // playback stepper re-derives packets from the shadow side.
        let consumed = (target - seed_tick) as usize;
        for i in 0..consumed {
            let tick = seed_tick + i as u32;
            if let Some(rr) = result.round_result {
                if tick >= rr.tick {
                    break;
                }
            }
            if let Some(writer) = self.replay_writer.lock().as_mut() {
                writer
                    .write_input(self.local_player_index, &self.pending_commits[i])
                    .expect("write input");
            }
        }
        for _ in 0..consumed {
            self.pending_commits.pop_front();
        }

        self.settled_state = Some(result.state);
        Ok(result.round_result)
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
        let fps_target = EXPECTED_FPS - slowdown;

        core.gba_mut()
            .sync_mut()
            .expect("set fps target")
            .set_fps_target(fps_target);
    }

    fn finalize_round(
        &mut self,
        round_result: Option<crate::stepper::RoundResult>,
        commit_frontier: u32,
    ) -> anyhow::Result<Option<BattleOutcome>> {
        let Some(round_result) = round_result else {
            return Ok(None);
        };
        if round_result.tick >= commit_frontier {
            return Ok(None);
        }

        log::info!(
            "round finished at {:x} (frontier {:x})",
            round_result.tick,
            self.frontier
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
        self.settled_state.is_some()
    }

    pub fn add_local_input(&mut self, input: PartialInput) {
        log::debug!("local input: {:?}", input);
        self.input_queue.add_local_input(input);
    }

    pub fn add_remote_input(&mut self, input: crate::net::Input) {
        log::debug!("remote input: {:?}", input);
        self.input_queue.add_remote_input(PartialInput {
            joyflags: input.joyflags,
        });
        self.last_remote_received_tick = self.last_remote_received_tick.wrapping_add(1);
        self.last_remote_frame_advantage = input.frame_advantage;
    }

    pub(super) fn can_add_remote_input(&self) -> bool {
        self.input_queue.can_add_remote_input()
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
