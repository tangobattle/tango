use std::sync::Arc;

use parking_lot::Mutex as PlMutex;
use tokio::sync::Mutex;

use crate::input::{Input, Pair, PairQueue, PartialInput};

use super::types::{BattleOutcome, CommittedState};
use super::EXPECTED_FPS;

/// Cap on the slowdown we'll apply (in fps). Only the leading peer
/// corrects — the trailing peer relies on the leader slowing down to
/// converge, which avoids both sides racing each other in opposite
/// directions when their skew estimates disagree about who's ahead.
const MAX_ADJUSTMENT: f32 = 30.0;

/// Per-frame EMA weight applied as `smoothed = α·sample + (1-α)·smoothed`,
/// asymmetric so the throttle prioritizes catching back up to full
/// speed over slowing down further.
///
/// `SLOWDOWN` (skew growing → need more slowdown) uses τ ≈ 5 s at
/// 60 Hz so a sub-second bursty-loss spike barely moves the smoothed
/// value: a 0.5 s spike of raw skew +30 contributes
/// ~30·(1-exp(-0.5/5)) ≈ +2.9, so the throttle barely engages under
/// heavy stochastic loss.
///
/// `SPEEDUP` (skew shrinking → can lift the slowdown) uses τ ≈ 0.5 s
/// so once the underlying imbalance closes, the local fps returns to
/// 60 within a handful of frames rather than coasting at the reduced
/// rate for the full slow τ. Net: the throttle ramps in gently but
/// recovers fast — the user sees a smooth glide down and a snappy
/// return to normal.
const SKEW_SMOOTH_ALPHA_SLOWDOWN: f32 = 1.0 / 300.0;
const SKEW_SMOOTH_ALPHA_SPEEDUP: f32 = 1.0 / 30.0;

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
    /// EMA of `local_adv - remote_adv`, updated every fastforward
    /// fire via [`SKEW_SMOOTH_ALPHA`]. Drives a continuous
    /// proportional slowdown of the leader (negative skew = trailer,
    /// no correction), so the fps target never steps and there's no
    /// engagement transient for the player to feel.
    smoothed_skew: f32,
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
}

impl Round {
    pub(super) fn new(
        match_: &super::Match,
        iq: PairQueue<PartialInput, PartialInput>,
    ) -> anyhow::Result<Self> {
        let hooks = match_.local_hooks();
        let stepper = crate::stepper::Fastforwarder::new(
            match_.rom(),
            hooks,
            match_.match_type(),
            match_.local_player_index(),
        )?;
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
            smoothed_skew: 0.0,
            iq,
            last_committed_remote_input,
            committed_state: None,
            stepper,
            replay_writer: match_.replay_writer_handle(),
            primary_thread_handle: match_.primary_thread_handle(),
            sender: match_.sender_handle(),
            shadow: match_.shadow_handle(),
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
        self.committed_state = Some(CommittedState {
            state: local_state,
            tick: 0,
            packet: first_packet.to_vec(),
        });
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
        let ff_result = self.run_fastforward(input_pairs, &last_committed_state, commit_tick, dirty_tick)?;

        self.commit_remote_inputs(&ff_result.output_pairs, last_committed_state.tick, commit_tick, ff_result.round_result);

        core.load_state(&ff_result.dirty_state.state).expect("load dirty state");
        self.committed_state = Some(ff_result.committed_state);
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
            })
            .await?;

        self.add_local_input(PartialInput { joyflags });
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
    /// ~τ (one-way delay) but matches what the throttle's skew estimate
    /// is reacting to.
    pub fn last_remote_frame_advantage(&self) -> i16 {
        self.last_remote_frame_advantage
    }

    fn prepare_input_pairs(
        &mut self,
    ) -> (Vec<Pair<PartialInput, PartialInput>>, CommittedState, u32, u32) {
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
    ) -> anyhow::Result<crate::stepper::FastforwardResult> {
        let shadow = self.shadow.clone();
        let hooks = self.hooks;
        let mut last_commit = self.last_committed_remote_input.packet.clone();
        self.stepper.fastforward(
            &last_committed_state.state,
            input_pairs,
            last_committed_state.tick,
            commit_tick,
            dirty_tick,
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

    fn update_fps_target(&mut self, mut core: mgba::core::CoreMutRef<'_>) {
        // Continuous EMA-filtered proportional time sync, asymmetric
        // (only the leading peer slows; trailer relies on the leader
        // pulling back to converge). `local_adv` and
        // `last_remote_frame_advantage` both carry the symmetric
        // network-delay term τ; their difference isolates real-time
        // clock skew. `local_adv_A + local_adv_B` is a network-fixed
        // invariant (60·RTT - 2·input_delay), so the only reachable
        // symmetric state is local_adv_A = local_adv_B = sum/2 →
        // raw_skew = 0, and the asymmetric correction can't have both
        // sides slowing simultaneously in equilibrium.
        //
        // The EMA replaces the old watchdog's trigger counter: a
        // bursty-loss spike that briefly inflates raw skew only nudges
        // the filtered value by a tiny fraction of its peak, so the
        // throttle reads through it the same way the deadband did —
        // but without the binary engagement step that made the
        // watchdog's transitions visible. Persistent skew converges
        // exponentially with τ ≈ 5 s; capped at MAX_ADJUSTMENT for
        // catastrophic rifts.
        let local_advantage = self.local_frame_advantage() as i32;
        let remote_advantage = self.last_remote_frame_advantage as i32;
        let skew = (local_advantage - remote_advantage) as f32;

        // Asymmetric α: slow rise (we slow down gradually) but fast
        // fall (we lift the slowdown quickly so we don't coast
        // unnecessarily after the imbalance resolves).
        let alpha = if skew > self.smoothed_skew {
            SKEW_SMOOTH_ALPHA_SLOWDOWN
        } else {
            SKEW_SMOOTH_ALPHA_SPEEDUP
        };
        self.smoothed_skew = alpha * skew + (1.0 - alpha) * self.smoothed_skew;
        // Only slow the leader. Negative smoothed skew = trailer, no
        // speed-up — the symmetric invariant guarantees the other side
        // sees positive skew and is the one being throttled.
        let adjustment = -self.smoothed_skew.max(0.0).min(MAX_ADJUSTMENT);

        let fps_target = (EXPECTED_FPS + adjustment).clamp(EXPECTED_FPS - MAX_ADJUSTMENT, EXPECTED_FPS);
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

    pub fn local_delay(&self) -> u32 {
        self.iq.local_delay()
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
