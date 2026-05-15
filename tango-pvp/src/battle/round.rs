use std::sync::Arc;

use parking_lot::Mutex as PlMutex;
use tokio::sync::Mutex;

use crate::input::{Input, Pair, PairQueue, PartialInput};

use super::types::{BattleOutcome, CommittedState};
use super::EXPECTED_FPS;

/// Per-round state for the live primary. Owns the input queue, the
/// committed state, the Fastforwarder dedicated to this round, and the
/// helpers that wire remote-side prediction into FF runs.
pub struct Round {
    hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
    number: u8,
    local_player_index: u8,
    current_tick: u32,
    /// Signed tick lag: how far ahead remote is of us. Positive when we're
    /// behind (need to speed up), negative when we're leading (slow down).
    /// Updated each frame from the most recent committed remote input's tick.
    tick_lag: i32,
    iq: PairQueue<PartialInput, PartialInput>,
    /// Joyflags + packet of the last committed remote input. Tick is tracked
    /// separately in `last_committed_remote_tick` because Inputs no longer
    /// carry a tick field.
    last_committed_remote_input: Input,
    last_committed_remote_tick: u32,
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
        number: u8,
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
            number,
            local_player_index: match_.local_player_index(),
            current_tick: 0,
            tick_lag: 0,
            iq,
            last_committed_remote_input,
            last_committed_remote_tick: 0,
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

        self.sender
            .lock()
            .await
            .send(&crate::net::Input {
                round_number: self.number,
                joyflags,
            })
            .await?;

        self.add_local_input(PartialInput { joyflags });
        Ok(())
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
            self.last_committed_remote_tick = tick;
        }
    }

    fn update_fps_target(&mut self, mut core: mgba::core::CoreMutRef<'_>) {
        // Positive when remote is ahead of us (we should speed up); negative
        // when we're ahead (we should slow down).
        self.tick_lag = self.last_committed_remote_tick as i32 - self.current_tick as i32;
        core.gba_mut()
            .sync_mut()
            .expect("set fps target")
            .set_fps_target((EXPECTED_FPS + self.tps_adjustment()).clamp(EXPECTED_FPS / 4.0, EXPECTED_FPS * 4.0));
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

    pub fn local_queue_length(&self) -> usize {
        self.iq.local_queue_length()
    }

    pub fn remote_queue_length(&self) -> usize {
        self.iq.remote_queue_length()
    }

    pub fn add_local_input(&mut self, input: PartialInput) {
        log::debug!("local input: {:?}", input);
        self.iq.add_local_input(input);
    }

    pub fn add_remote_input(&mut self, input: PartialInput) {
        log::debug!("remote input: {:?}", input);
        self.iq.add_remote_input(input);
    }

    pub(super) fn can_add_remote_input(&self) -> bool {
        self.iq.can_add_remote_input()
    }

    pub fn tps_adjustment(&self) -> f32 {
        // tanh shaping bounds the adjustment to ±EXPECTED_FPS/2, so fps_target stays
        // in roughly [EXPECTED_FPS/2, EXPECTED_FPS*1.5] no matter how far we diverge.
        // Without a bound, large |tick_lag| pushes fps_target past zero and breaks throttling.
        let max = EXPECTED_FPS * 0.5;
        max * (self.tick_lag as f32 / 8.0).tanh()
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
