use crate::hooks;
use crate::input;

struct InnerState {
    current_tick: u32,
    local_player_index: u8,
    input_pairs: std::collections::VecDeque<input::Pair<input::Input, input::Input>>,
    consumed_input_pairs: Vec<input::Pair<input::Input, input::Input>>,
    commit_time: u32,
    committed_state: Option<mgba::state::State>,
    dirty_time: u32,
    dirty_state: Option<mgba::state::State>,
    round_result: Option<BattleResult>,
    round_end_time: Option<u32>,
    error: Option<anyhow::Error>,
}

pub struct FastforwardResult {
    pub committed_state: mgba::state::State,
    pub dirty_state: mgba::state::State,
    pub last_input: input::Pair<input::Input, input::Input>,
    pub consumed_input_pairs: Vec<input::Pair<input::Input, input::Input>>,
}

#[derive(Clone, Copy, serde_repr::Serialize_repr)]
#[repr(i8)]
pub enum BattleResult {
    Draw = -1,
    Loss = 0,
    Win = 1,
}

pub struct Fastforwarder {
    core: mgba::core::Core,
    state: State,
    hooks: &'static Box<dyn hooks::Hooks + Send + Sync>,
    local_player_index: u8,
}

#[derive(Clone)]
pub struct State(std::sync::Arc<parking_lot::Mutex<Option<InnerState>>>);

impl State {
    pub fn new(
        local_player_index: u8,
        input_pairs: Vec<input::Pair<input::Input, input::Input>>,
        commit_time: u32,
    ) -> State {
        State(std::sync::Arc::new(parking_lot::Mutex::new(Some(
            InnerState {
                current_tick: 0,
                local_player_index,
                input_pairs: input_pairs.into_iter().collect(),
                consumed_input_pairs: vec![],
                commit_time,
                committed_state: None,
                dirty_time: 0,
                dirty_state: None,
                round_result: None,
                round_end_time: None,
                error: None,
            },
        ))))
    }

    pub fn take_error(&self) -> Option<anyhow::Error> {
        self.0.lock().as_mut().expect("error").error.take()
    }

    pub fn commit_time(&self) -> u32 {
        self.0.lock().as_ref().expect("commit time").commit_time
    }

    pub fn set_round_result(&self, result: BattleResult) {
        self.0.lock().as_mut().expect("round result").round_result = Some(result);
    }

    pub fn round_result(&self) -> Option<BattleResult> {
        self.0.lock().as_ref().expect("round result").round_result
    }

    pub fn set_committed_state(&self, state: mgba::state::State) {
        self.0
            .lock()
            .as_mut()
            .expect("committed state")
            .committed_state = Some(state);
    }

    pub fn take_committed_state(&self) -> Option<mgba::state::State> {
        self.0
            .lock()
            .as_mut()
            .expect("committed state")
            .committed_state
            .take()
    }

    pub fn dirty_time(&self) -> u32 {
        self.0.lock().as_ref().expect("dirty time").dirty_time
    }

    pub fn set_dirty_state(&self, state: mgba::state::State) {
        self.0.lock().as_mut().expect("dirty state").dirty_state = Some(state);
    }

    pub fn peek_input_pair(&self) -> Option<input::Pair<input::Input, input::Input>> {
        self.0
            .lock()
            .as_ref()
            .expect("input pairs")
            .input_pairs
            .front()
            .cloned()
    }

    pub fn pop_input_pair(&self) -> Option<input::Pair<input::Input, input::Input>> {
        let mut inner = self.0.lock();
        let inner = inner.as_mut().expect("input pairs");
        let ip = inner.input_pairs.pop_front();
        if let Some(ip) = ip.clone() {
            inner.consumed_input_pairs.push(ip);
        }
        ip
    }

    pub fn set_anyhow_error(&self, err: anyhow::Error) {
        self.0.lock().as_mut().expect("error").error = Some(err);
    }

    pub fn local_player_index(&self) -> u8 {
        self.0
            .lock()
            .as_ref()
            .expect("local player index")
            .local_player_index
    }

    pub fn remote_player_index(&self) -> u8 {
        1 - self.local_player_index()
    }

    pub fn are_inputs_exhausted(&self) -> bool {
        self.0
            .lock()
            .as_ref()
            .expect("are inputs exhausted")
            .input_pairs
            .is_empty()
    }

    pub fn end_round(&self) {
        let mut inner = self.0.lock();
        let inner = inner.as_mut().expect("on battle ended");
        inner.round_end_time = Some(inner.current_tick);
    }

    pub fn round_end_time(&self) -> Option<u32> {
        self.0
            .lock()
            .as_ref()
            .expect("on battle ended")
            .round_end_time
    }

    pub fn inputs_pairs_left(&self) -> usize {
        self.0
            .lock()
            .as_ref()
            .expect("input pairs")
            .input_pairs
            .len()
    }

    pub fn current_tick(&self) -> u32 {
        self.0.lock().as_ref().expect("current tick").current_tick
    }

    pub fn increment_current_tick(&self) {
        self.0.lock().as_mut().expect("current tick").current_tick += 1;
    }
}

impl Fastforwarder {
    pub fn new(
        rom: &[u8],
        hooks: &'static Box<dyn hooks::Hooks + Send + Sync>,
        local_player_index: u8,
        opponent_nickname: &Option<String>,
    ) -> anyhow::Result<Self> {
        let mut core = mgba::core::Core::new_gba("tango")?;
        let rom_vf = mgba::vfile::VFile::open_memory(rom);
        core.as_mut().load_rom(rom_vf)?;
        hooks.patch(core.as_mut());

        let state = State(std::sync::Arc::new(parking_lot::Mutex::new(None)));

        let mut traps = hooks.common_traps();
        traps.extend(hooks.replayer_traps(state.clone()));
        core.set_traps(traps);
        if let Some(opponent_nickname) = opponent_nickname.as_ref() {
            hooks.replace_opponent_name(core.as_mut(), opponent_nickname);
        };
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
        last_committed_tick: u32,
        commit_pairs: &[input::Pair<input::Input, input::Input>],
        last_committed_remote_input: input::Input,
        local_player_inputs_left: &[input::Input],
    ) -> anyhow::Result<FastforwardResult> {
        let mut predicted_rx = last_committed_remote_input.rx.clone();
        let input_pairs = commit_pairs
            .iter()
            .cloned()
            .chain(local_player_inputs_left.iter().cloned().map(|local| {
                let local_tick = local.local_tick;
                let remote_tick = local.remote_tick;
                self.hooks.predict_rx(&mut predicted_rx);
                input::Pair {
                    local,
                    remote: input::Input {
                        local_tick,
                        remote_tick,
                        joyflags: {
                            let mut joyflags = 0;
                            if last_committed_remote_input.joyflags & mgba::input::keys::A as u16
                                != 0
                            {
                                joyflags |= mgba::input::keys::A as u16;
                            }
                            if last_committed_remote_input.joyflags & mgba::input::keys::B as u16
                                != 0
                            {
                                joyflags |= mgba::input::keys::B as u16;
                            }
                            joyflags
                        },
                        rx: predicted_rx.clone(),
                        is_prediction: true,
                    },
                }
            }))
            .collect::<Vec<input::Pair<input::Input, input::Input>>>();
        let last_input = input_pairs.last().expect("last input pair").clone();

        self.core.as_mut().load_state(state)?;
        self.hooks.prepare_for_fastforward(self.core.as_mut());

        let commit_time = last_committed_tick + commit_pairs.len() as u32;
        let dirty_time = last_committed_tick + input_pairs.len() as u32 - 1;

        *self.state.0.lock() = Some(InnerState {
            current_tick: last_committed_tick,
            local_player_index: self.local_player_index,
            input_pairs: input_pairs.into_iter().collect(),
            consumed_input_pairs: vec![],
            commit_time,
            committed_state: None,
            dirty_time,
            dirty_state: None,
            round_result: None,
            round_end_time: None,
            error: None,
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
                        consumed_input_pairs: state.consumed_input_pairs,
                        last_input,
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
