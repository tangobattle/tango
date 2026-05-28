use std::sync::Arc;

use parking_lot::Mutex as PlMutex;
use tokio::sync::Mutex;

use crate::input::{Input, Pair, PairQueue, PartialInput};

use super::throttler::Throttler;
use super::types::{BattleOutcome, CommittedState};
use super::EXPECTED_FPS;

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
    /// Tick of the last `present_state` loaded into the live core (0 before
    /// any load — same as the game's initial tick, so callers don't need a
    /// special pre-load case). The live core's game tick advances naturally
    /// from here, so per-game `primary` traps that need the game's tick read
    /// [`Self::last_loaded_tick`] and add `+ 1` once
    /// `round_post_increment_tick` has fired. Decoupled from `current_tick`
    /// (the netcode frontier), which advances at wall-clock rate via the
    /// post-tick hook regardless of how far the live core lags.
    last_loaded_tick: u32,
    /// Local presentation delay in frames (`frontier − presentation_delay` is
    /// the display's target tick). Fixed for the match; the netcode-affecting
    /// part of this side's requested `frame_delay` lives in `input_delay`.
    presentation_delay: u32,
    /// Shared input delay in frames (`min` of the two peers' `frame_delay`).
    /// Local input is delay-lined by this much before it's queued, and the
    /// remote queue is seeded with this many neutral inputs at round start, so
    /// the committed frontier sits `input_delay` closer to the live frontier —
    /// i.e. rollback depth drops by `input_delay`. Symmetric, so it's "fair".
    input_delay: u32,
    /// Depth-`input_delay` ring delaying the local joyflags before they enter
    /// the queue: the input committed at tick T is the one sampled at T −
    /// `input_delay` (neutral for the first `input_delay` ticks). Pre-seeded
    /// with `input_delay` neutral frames. The value *sent on the wire* is the
    /// raw, un-delayed sample — the peer's matching remote-queue prefill is what
    /// realizes the delay on their side.
    local_input_delay_line: std::collections::VecDeque<u16>,
    /// The single settled checkpoint the present roll advances. Lags
    /// the frontier by ~`presentation_delay`; advances one tick per frame (it only
    /// moves forward, since the frontier does). `None` until the round's first
    /// commit seeds it.
    present_seed: Option<CommittedState>,
    /// Confirmed input pairs (real remote packet included) not yet consumed by
    /// the rolling seed: a sliding window `[confirmed_base, commit_frontier)`.
    /// Front entries are dropped as the seed rolls past them, so this stays
    /// small (~`presentation_delay` deep) instead of growing for the whole round.
    confirmed: std::collections::VecDeque<Pair<Input, Input>>,
    /// Tick of `confirmed.front()` — lets us index the deque by absolute tick.
    confirmed_base: u32,
    /// Speculative depth of the most recent frame: local inputs queued past
    /// the commit frontier (= `peeked.len()` after this frame's
    /// `consume_and_peek_local`). The number of frames a real remote packet
    /// can force us to re-simulate when it arrives. Surfaced in the status
    /// bar as "depth".
    last_rollback_depth: u32,
}

impl Round {
    pub(super) fn new(match_: &super::Match, mut iq: PairQueue<PartialInput, PartialInput>) -> anyhow::Result<Self> {
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
        // ahead (the early ticks are free neutrals), so the confirmed frontier
        // reaches `input_delay` further than one-per-frame arrivals alone would
        // — that's the rollback reduction. These prefilled neutrals must NOT
        // bump `last_remote_received_tick` (the throttler's metric tracks the
        // *real* network relationship), so push them straight onto the queue
        // rather than through `add_remote_input`.
        for _ in 0..input_delay {
            iq.add_remote_input(PartialInput { joyflags: 0 });
        }

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
            last_loaded_tick: 0,
            presentation_delay: match_.presentation_delay(),
            input_delay,
            // Pre-seeded with `input_delay` neutral frames so the first
            // `input_delay` committed local inputs are neutral, mirroring the
            // remote prefill above.
            local_input_delay_line: std::collections::VecDeque::from(vec![0u16; input_delay as usize]),
            present_seed: None,
            confirmed: std::collections::VecDeque::new(),
            confirmed_base: 0,
            last_rollback_depth: 0,
        })
    }

    /// Netcode frontier — advances one per wall-frame via the post-tick hook,
    /// regardless of how far the live core lags. Internal to the crate; per-game
    /// traps that need a tick should reach for [`Self::last_loaded_tick`]
    /// (the game's tick) instead.
    pub(crate) fn current_tick(&self) -> u32 {
        self.current_tick
    }

    /// Tick of the last `present_state` we loaded into the live core, or `None`
    /// before any load. The live core's game tick is this value while
    /// in-tick processing (between `main_read_joyflags` and
    /// `round_post_increment_tick`), and this value `+ 1` once
    /// `round_post_increment_tick` has fired (i.e. at the next
    /// `main_read_joyflags`). Decoupled from `current_tick` (the netcode
    /// frontier, which runs `presentation_delay` ticks ahead). Per-game
    /// primary traps that need the game's tick (not the netcode frontier)
    /// read this and apply the appropriate `+ 1` for the phase they fire in.
    pub fn last_loaded_tick(&self) -> u32 {
        self.last_loaded_tick
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
        self.send_and_queue_local_input(joyflags).await?;

        let (committable, peeked) = self.iq.consume_and_peek_local();
        let last_committed_state = self.committed_state.take().expect("committed state");
        let last_committed_tick = last_committed_state.tick;
        let commit_tick = last_committed_tick + committable.len() as u32;
        self.last_rollback_depth = peeked.len() as u32;

        let predicted_joyflags = predicted_remote_joyflags(self.last_committed_remote_input.joyflags);

        // Mini-FF: advance `committed_state` through *only* the newly-committable
        // range. One stepper tick per new commit; the trap calls `shadow.apply_input`
        // via the callback, advancing shadow in lockstep. The speculative range is
        // handled by the fresh speculative-tail FF below — so this FF saves
        // `rollback_depth` ticks of emulation per frame vs the old wide main FF.
        let (new_committed_state, output_pairs, round_result) = if committable.is_empty() {
            (last_committed_state, vec![], None)
        } else {
            let shadow = self.shadow.clone();
            let mut last_commit = self.last_committed_remote_input.packet.clone();
            // dirty_tick = commit_tick - 1 lets the FF exit at the last
            // committable's tick while the commit capture still fires on the
            // trailing main_read_joyflags at game.current_tick == commit_tick
            // (the per-game trap captures committed_state before peeking the
            // next input, so it works even with no input left to peek).
            let result = self.stepper.fastforward(
                &last_committed_state.state,
                committable,
                last_committed_state.tick,
                commit_tick,
                commit_tick.saturating_sub(1),
                u32::MAX,
                &last_committed_state.packet,
                Box::new(move |tick, ip| {
                    let packet = shadow.lock().apply_input(tick, ip)?;
                    last_commit.clone_from(&packet);
                    Ok(packet)
                }),
            )?;
            (result.committed_state, result.output_pairs, result.round_result)
        };

        self.commit_remote_inputs(&output_pairs, last_committed_tick, commit_tick, round_result);

        // Display target. `current_tick` is the netcode frontier (advances 1/wall-frame
        // via the post-tick hook regardless of where the live core is loaded), so
        // `current_tick - presentation_delay` is what the user sees.
        let target = self.current_tick.saturating_sub(self.presentation_delay);

        // Roll the settled checkpoint forward, *capped* at the last committed
        // tick (`commit_tick - 1`). The seed must never absorb predicted
        // packets — when commit_tick later catches up, the real packets at
        // those ticks differ from what predict_rx produced, leaving the seed
        // stale and snowballing the error every subsequent frame. The
        // speculative tail is re-simulated fresh below.
        let settled_target = target.min(commit_tick.saturating_sub(1));
        let rolled = self.roll_present_to(settled_target)?;

        let present_state = if target <= settled_target {
            // Settled regime (presentation_delay >= rollback_depth): the rolled
            // seed IS the display.
            rolled
        } else {
            // Speculative tail: fresh sim from `new_committed_state` through the
            // peeked locals + a predicted remote, capturing the state at `target`.
            // Range = `target - commit_tick + 1` ticks (= rollback_depth -
            // presentation_delay in steady state), which is what the live core
            // shows when the user sees beyond the commit frontier.
            let hooks = self.hooks;
            // Pre-apply `predict_rx` once so the seed already represents the
            // packet at `commit_tick` — the callback then returns-then-predicts,
            // i.e. on entry `predict_packet` always represents the *current*
            // tick's packet (cleaner invariant than predict-then-return).
            let mut predict_packet = self.last_committed_remote_input.packet.clone();
            hooks.predict_rx(&mut predict_packet);
            let count = (target - commit_tick + 1) as usize;
            let input_pairs: Vec<Pair<PartialInput, PartialInput>> = peeked[..count]
                .iter()
                .map(|local| Pair {
                    local: local.clone(),
                    remote: PartialInput {
                        joyflags: predicted_joyflags,
                    },
                })
                .collect();
            let result = self.stepper.fastforward(
                &new_committed_state.state,
                input_pairs,
                commit_tick,
                target,
                target,
                u32::MAX, // dirty_state at `target` IS the present
                &new_committed_state.packet,
                Box::new(move |_tick, _ip| {
                    let out = predict_packet.clone();
                    hooks.predict_rx(&mut predict_packet);
                    Ok(out)
                }),
            )?;
            result.dirty_state.state
        };

        self.committed_state = Some(new_committed_state);
        core.load_state(&present_state).expect("load present state");
        self.last_loaded_tick = target;
        self.update_fps_target(core);

        self.finalize_round(round_result, commit_tick)
    }

    async fn send_and_queue_local_input(&mut self, joyflags: u16) -> anyhow::Result<()> {
        if !self.iq.can_add_local_input() {
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
        let diff = self.current_tick as i32 - self.last_remote_received_tick as i32;
        diff.clamp(i16::MIN as i32, i16::MAX as i32) as i16
    }

    /// Peer's frame advantage as of their most recent packet — stale by
    /// ~τ (one-way delay) but matches what the throttler's skew estimate
    /// is reacting to.
    pub fn last_remote_frame_advantage(&self) -> i16 {
        self.last_remote_frame_advantage
    }

    /// Speculative depth of the most recent FF run — the number of frames a
    /// real remote packet can force us to roll back and re-simulate.
    pub fn rollback_depth(&self) -> u32 {
        self.last_rollback_depth
    }

    /// Shared input delay applied this match (`min` of the two peers'
    /// frame_delay). Constant for the match; this much was already shaved off
    /// `rollback_depth` versus a pure-presentation-delay setup.
    pub fn input_delay(&self) -> u32 {
        self.input_delay
    }

    /// Roll the single settled checkpoint forward to `target` (which is at or
    /// behind the commit frontier) and return the state there for the display.
    /// Replays the confirmed inputs from the seed's tick over the present
    /// stepper — no shadow (packets come from the confirmed window) and no
    /// prediction (every tick up to `target` is committed). The seed only ever
    /// moves forward; if `target` is at or behind it (early in the round, before
    /// the frontier has advanced `presentation_delay` ticks, or no advance yet)
    /// we hold the current frame.
    fn roll_present_to(&mut self, target: u32) -> anyhow::Result<Box<mgba::state::State>> {
        let seed_tick = self.present_seed.as_ref().expect("present seed").tick;
        if target <= seed_tick {
            // Hold (don't roll backward): re-show the current checkpoint while
            // the frontier climbs to `presentation_delay` ticks past the round's
            // start (the present can't move until then).
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
        let fps_target = EXPECTED_FPS - slowdown;

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
