use crate::battle;
use crate::game;
use crate::lockstep;

pub struct InnerState {
    current_tick: u32,
    local_player_index: u8,
    input_pairs:
        std::collections::VecDeque<lockstep::Pair<lockstep::PartialInput, lockstep::PartialInput>>,
    output_pairs: Vec<lockstep::Pair<lockstep::Input, lockstep::Input>>,
    apply_shadow_input: Box<
        dyn FnMut(
                lockstep::Pair<lockstep::Input, lockstep::PartialInput>,
            ) -> anyhow::Result<Vec<u8>>
            + Sync
            + Send,
    >,
    local_packet: Option<lockstep::Packet>,
    commit_tick: u32,
    committed_state: Option<battle::CommittedState>,
    dirty_tick: u32,
    dirty_state: Option<battle::CommittedState>,
    round_result: Option<RoundResult>,
    phase: RoundPhase,
    on_round_ended: Option<Box<dyn FnOnce() + Send>>,
    error: Option<anyhow::Error>,
}

impl InnerState {
    pub fn take_error(&mut self) -> Option<anyhow::Error> {
        self.error.take()
    }

    pub fn commit_tick(&self) -> u32 {
        self.commit_tick
    }

    pub fn set_round_result(&mut self, result: BattleResult) {
        self.round_result = Some(RoundResult {
            tick: self.current_tick,
            result,
        });
    }

    pub fn set_committed_state(&mut self, state: mgba::state::State) {
        let local_packet = self.local_packet.clone().unwrap();
        if self.current_tick != local_packet.tick {
            panic!(
                "local packet tick mismatch: {} != {}",
                self.current_tick, local_packet.tick
            );
        }
        self.committed_state = Some(battle::CommittedState {
            tick: self.current_tick,
            state,
            packet: local_packet.packet,
        });
    }

    pub fn take_committed_state(&mut self) -> Option<battle::CommittedState> {
        self.committed_state.take()
    }

    pub fn dirty_tick(&self) -> u32 {
        self.dirty_tick
    }

    pub fn set_dirty_state(&mut self, state: mgba::state::State) {
        let local_packet = self.local_packet.clone().unwrap();
        if self.current_tick != local_packet.tick {
            panic!(
                "local packet tick mismatch: {} != {}",
                self.current_tick, local_packet.tick
            );
        }
        self.dirty_state = Some(battle::CommittedState {
            tick: self.current_tick,
            state,
            packet: local_packet.packet,
        });
    }

    pub fn peek_input_pair(
        &self,
    ) -> Option<&lockstep::Pair<lockstep::PartialInput, lockstep::PartialInput>> {
        self.input_pairs.front()
    }

    pub fn pop_input_pair(
        &mut self,
    ) -> Option<lockstep::Pair<lockstep::PartialInput, lockstep::PartialInput>> {
        self.input_pairs.pop_front()
    }

    pub fn apply_shadow_input(
        &mut self,
        input: lockstep::Pair<lockstep::Input, lockstep::PartialInput>,
    ) -> anyhow::Result<Vec<u8>> {
        let remote_packet = (self.apply_shadow_input)(input.clone())?;
        self.output_pairs.push(lockstep::Pair {
            local: input.local,
            remote: input.remote.with_packet(remote_packet.clone()),
        });
        Ok(remote_packet)
    }

    pub fn set_local_packet(&mut self, tick: u32, packet: Vec<u8>) {
        self.local_packet = Some(lockstep::Packet { tick, packet });
    }

    pub fn peek_local_packet(&mut self) -> Option<&lockstep::Packet> {
        self.local_packet.as_ref()
    }

    pub fn set_anyhow_error(&mut self, err: anyhow::Error) {
        self.error = Some(err);
    }

    pub fn local_player_index(&self) -> u8 {
        self.local_player_index
    }

    pub fn remote_player_index(&self) -> u8 {
        1 - self.local_player_index()
    }

    pub fn set_round_ending(&mut self) {
        self.phase = RoundPhase::Ending;
    }

    pub fn set_round_ended(&mut self) {
        self.phase = RoundPhase::Ended;
        if let Some(on_round_ended) = self.on_round_ended.take() {
            on_round_ended();
        }
    }

    pub fn is_round_ending(&self) -> bool {
        self.phase == RoundPhase::Ending || self.phase == RoundPhase::Ended
    }

    pub fn is_round_ended(&self) -> bool {
        self.phase == RoundPhase::Ended
    }

    pub fn round_result(&self) -> Option<RoundResult> {
        self.round_result
    }

    pub fn input_pairs_left(&self) -> usize {
        self.input_pairs.len()
    }

    pub fn current_tick(&self) -> u32 {
        self.current_tick
    }

    pub fn increment_current_tick(&mut self) {
        self.current_tick += 1;
    }
}

pub struct FastforwardResult {
    pub committed_state: battle::CommittedState,
    pub dirty_state: battle::CommittedState,
    pub round_result: Option<RoundResult>,
    pub output_pairs: Vec<lockstep::Pair<lockstep::Input, lockstep::Input>>,
}

#[derive(Clone, Copy, serde_repr::Serialize_repr)]
#[repr(i8)]
pub enum BattleResult {
    Draw = -1,
    Loss = 0,
    Win = 1,
}

#[derive(Clone, Copy, PartialEq)]
enum RoundPhase {
    InProgress,
    Ending,
    Ended,
}

#[derive(Clone, Copy)]
pub struct RoundResult {
    pub tick: u32,
    pub result: BattleResult,
}

pub struct Fastforwarder {
    core: mgba::core::Core,
    state: State,
    hooks: &'static (dyn game::Hooks + Send + Sync),
    local_player_index: u8,
}

#[derive(Clone)]
pub struct State(std::sync::Arc<parking_lot::Mutex<Option<InnerState>>>);

impl State {
    pub fn new(
        local_player_index: u8,
        input_pairs: Vec<lockstep::Pair<lockstep::Input, lockstep::Input>>,
        commit_tick: u32,
        on_round_ended: Box<dyn FnOnce() + Send>,
    ) -> State {
        let local_packet = input_pairs.first().map(|ip| lockstep::Packet {
            tick: ip.local.local_tick,
            packet: ip.local.packet.clone(),
        });
        State(std::sync::Arc::new(parking_lot::Mutex::new(Some(
            InnerState {
                current_tick: 0,
                local_player_index,
                input_pairs: input_pairs
                    .iter()
                    .map(|ip| lockstep::Pair {
                        local: lockstep::PartialInput {
                            local_tick: ip.local.local_tick,
                            remote_tick: ip.local.remote_tick,
                            joyflags: ip.local.joyflags,
                        },
                        remote: lockstep::PartialInput {
                            local_tick: ip.remote.local_tick,
                            remote_tick: ip.remote.remote_tick,
                            joyflags: ip.remote.joyflags,
                        },
                    })
                    .collect(),
                apply_shadow_input: Box::new({
                    let mut iq = input_pairs
                        .into_iter()
                        .collect::<std::collections::VecDeque<_>>();
                    move |_| {
                        let ip = if let Some(ip) = iq.pop_front() {
                            ip
                        } else {
                            anyhow::bail!("no more committed inputs");
                        };
                        Ok(ip.remote.packet)
                    }
                }),
                output_pairs: vec![],
                local_packet,
                commit_tick,
                committed_state: None,
                dirty_tick: 0,
                dirty_state: None,
                round_result: None,
                phase: RoundPhase::InProgress,
                error: None,
                on_round_ended: Some(on_round_ended),
            },
        ))))
    }

    pub fn lock_inner(&self) -> parking_lot::MappedMutexGuard<'_, InnerState> {
        parking_lot::MutexGuard::map(self.0.lock(), |s| s.as_mut().unwrap())
    }
}

impl Fastforwarder {
    pub fn new(
        rom: &[u8],
        hooks: &'static (dyn game::Hooks + Send + Sync),
        local_player_index: u8,
    ) -> anyhow::Result<Self> {
        let mut core = mgba::core::Core::new_gba("tango")?;
        let rom_vf = mgba::vfile::VFile::open_memory(rom);
        core.as_mut().load_rom(rom_vf)?;
        hooks.patch(core.as_mut());

        let state = State(std::sync::Arc::new(parking_lot::Mutex::new(None)));

        let mut traps = hooks.common_traps();
        traps.extend(hooks.replayer_traps(state.clone()));
        core.set_traps(traps);
        core.as_mut().reset();

        Ok(Fastforwarder {
            core,
            state,
            hooks,
            local_player_index,
        })
    }

    pub fn fastforward(
        &mut self,
        state: &mgba::state::State,
        input_pairs: Vec<lockstep::Pair<lockstep::PartialInput, lockstep::PartialInput>>,
        current_tick: u32,
        commit_tick: u32,
        dirty_tick: u32,
        last_local_packet: &[u8],
        apply_shadow_input: Box<
            dyn FnMut(
                    lockstep::Pair<lockstep::Input, lockstep::PartialInput>,
                ) -> anyhow::Result<Vec<u8>>
                + Sync
                + Send,
        >,
    ) -> anyhow::Result<FastforwardResult> {
        self.core.as_mut().load_state(state)?;
        self.hooks.prepare_for_fastforward(self.core.as_mut());

        *self.state.0.lock() = Some(InnerState {
            current_tick,
            local_player_index: self.local_player_index,
            input_pairs: input_pairs.into_iter().collect(),
            output_pairs: vec![],
            apply_shadow_input,
            local_packet: Some(lockstep::Packet {
                tick: current_tick,
                packet: last_local_packet.to_vec(),
            }),
            commit_tick,
            committed_state: None,
            dirty_tick,
            dirty_state: None,
            round_result: None,
            phase: RoundPhase::InProgress,
            error: None,
            on_round_ended: Some(Box::new(|| {})),
        });

        loop {
            {
                let mut inner_state_guard = self.state.0.lock();
                let mut inner_state = inner_state_guard.as_mut().unwrap();
                if inner_state.committed_state.is_some() && inner_state.dirty_state.is_some() {
                    let state = inner_state_guard.take().expect("state");
                    return Ok(FastforwardResult {
                        committed_state: state.committed_state.expect("committed state"),
                        dirty_state: state.dirty_state.expect("dirty state"),
                        round_result: state.round_result,
                        output_pairs: state.output_pairs,
                    });
                }
                inner_state.error = None;
            }
            self.core.as_mut().run_loop();
            let mut inner_state = self.state.0.lock();
            if let Some(_) = inner_state.as_ref().expect("state").error {
                let state = inner_state.take().expect("state");
                return Err(anyhow::format_err!(
                    "replayer: {}",
                    state.error.expect("error")
                ));
            }
        }
    }
}
