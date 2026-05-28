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
    /// Tick of the last `present_state` loaded into the live core, or `None`
    /// before the first load. The live core's game tick advances naturally from
    /// here, so the per-game `primary` trap verifies
    /// `game.current_tick == last_loaded_tick + 1` (see
    /// [`Self::expected_game_tick`]). Decoupled from `current_tick` (the
    /// netcode frontier), which advances at wall-clock rate via the post-tick
    /// hook regardless of how far the live core lags.
    last_loaded_tick: Option<u32>,
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
    /// Speculative depth of the most recent FF run: how many ticks past the
    /// last committed input we ran on prediction (`dirty_tick − commit_tick +
    /// 1`). This is the number of frames a real remote packet can force us to
    /// roll back and re-simulate. Surfaced in the status bar as "depth".
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
            last_loaded_tick: None,
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

    pub fn current_tick(&self) -> u32 {
        self.current_tick
    }

    /// What the live core's game tick should be at the next per-game `primary`
    /// trap fire: one past the last `present_state` we loaded (which is where
    /// the game resumed from), or 0 before any load. Decoupled from
    /// `current_tick`, which is the netcode frontier and runs `presentation_delay`
    /// ticks ahead. Per-game `primary` traps that verify the game's tick should
    /// compare against this, not `current_tick`.
    pub fn expected_game_tick(&self) -> u32 {
        self.last_loaded_tick.map_or(0, |t| t + 1)
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

        let (input_pairs, last_committed_state, commit_tick, dirty_tick) = self.prepare_input_pairs();
        self.last_rollback_depth = (dirty_tick + 1).saturating_sub(commit_tick);
        // Display target: the tick the display core renders, `frontier -
        // presentation_delay`.
        let present_target = dirty_tick.saturating_sub(self.presentation_delay);
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

        self.committed_state = Some(ff_result.committed_state);

        let target = present_target;
        // Roll the settled checkpoint forward, clamped to the last
        // *committed* tick (commit_tick is the frontier of committed
        // inputs, exclusive — there's no confirmed input there yet).
        let rolled = self.roll_present_to(target.min(commit_tick.saturating_sub(1)))?;
        // In settled territory the rolled checkpoint IS the present; in the
        // rarer speculative range (presentation_delay < rollback window) the live
        // FF captured the speculative present and the roll just keeps the
        // checkpoint current for when it settles again.
        let present_state = if target < commit_tick {
            rolled
        } else {
            ff_result.present_state.expect("live FF captures the speculative present")
        };
        // Single-core PvP: the live core loads the rendered `present_state`
        // (not the speculative frontier — `dirty_state` is unused on the live
        // side now). Its game tick lags `current_tick` by `presentation_delay`;
        // the per-game `primary` trap verifies against [`expected_game_tick`]
        // to account for this. `current_tick` (the netcode clock) still
        // advances at wall rate via the post-tick hook, decoupled from where
        // the live core happens to be loaded.
        core.load_state(&present_state).expect("load present state");
        self.last_loaded_tick = Some(present_target);
        self.update_fps_target(core);

        self.finalize_round(ff_result.round_result, commit_tick)
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
        // (`target >= commit_tick` — presentation_delay below the rollback window):
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
