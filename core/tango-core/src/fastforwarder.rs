use crate::hooks;
use crate::input;

struct InnerState {
    current_tick: u32,
    local_player_index: u8,
    input_pairs: std::collections::VecDeque<input::Pair<input::Input, input::Input>>,
    commit_time: u32,
    committed_state: Option<mgba::state::State>,
    dirty_time: u32,
    dirty_state: Option<mgba::state::State>,
    on_inputs_exhausted: Box<dyn Fn() + Send>,
    on_battle_ended: Box<dyn Fn() + Send>,
    error: Option<anyhow::Error>,
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
        on_inputs_exhausted: Box<dyn Fn() + Send>,
        on_battle_ended: Box<dyn Fn() + Send>,
    ) -> State {
        State(std::sync::Arc::new(parking_lot::Mutex::new(Some(
            InnerState {
                current_tick: 0,
                local_player_index,
                input_pairs: input_pairs.into_iter().collect(),
                commit_time: 0,
                committed_state: None,
                dirty_time: 0,
                dirty_state: None,
                on_inputs_exhausted,
                on_battle_ended,
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

    pub fn set_committed_state(&self, state: mgba::state::State) {
        self.0
            .lock()
            .as_mut()
            .expect("committed state")
            .committed_state = Some(state);
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
        self.0
            .lock()
            .as_mut()
            .expect("input pairs")
            .input_pairs
            .pop_front()
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

    pub fn on_inputs_exhausted(&self) {
        (self
            .0
            .lock()
            .as_mut()
            .expect("on inputs exhausted")
            .on_inputs_exhausted)();
    }

    pub fn on_battle_ended(&self) {
        (self
            .0
            .lock()
            .as_mut()
            .expect("on battle ended")
            .on_battle_ended)();
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
        rom_path: &std::path::Path,
        hooks: &'static Box<dyn hooks::Hooks + Send + Sync>,
        local_player_index: u8,
        opponent_nickname: &Option<String>,
    ) -> anyhow::Result<Self> {
        let mut core = mgba::core::Core::new_gba("tango")?;
        let rom_vf = mgba::vfile::VFile::open(rom_path, mgba::vfile::flags::O_RDONLY)?;
        core.as_mut().load_rom(rom_vf)?;

        let state = State(std::sync::Arc::new(parking_lot::Mutex::new(None)));

        core.set_traps(hooks.fastforwarder_traps(state.clone()));
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
        local_player_inputs_left: &[input::PartialInput],
    ) -> anyhow::Result<(
        mgba::state::State,
        mgba::state::State,
        input::Pair<input::Input, input::Input>,
    )> {
        let input_pairs = commit_pairs
            .iter()
            .cloned()
            .chain(local_player_inputs_left.iter().cloned().map(|local| {
                let local_tick = local.local_tick;
                let remote_tick = local.remote_tick;
                input::Pair {
                    local: input::Input {
                        local_tick,
                        remote_tick,
                        joyflags: local.joyflags,
                        rx: self.hooks.placeholder_rx(),
                    },
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
                        rx: last_committed_remote_input.rx.clone(),
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
            commit_time,
            committed_state: None,
            dirty_time,
            dirty_state: None,
            on_inputs_exhausted: Box::new(|| {}),
            on_battle_ended: Box::new(|| {}),
            error: None,
        });

        loop {
            {
                let mut inner_state_guard = self.state.0.lock();
                let mut inner_state = inner_state_guard.as_mut().unwrap();
                if inner_state.committed_state.is_some() && inner_state.dirty_state.is_some() {
                    let state = inner_state_guard.take().expect("state");
                    return Ok((
                        state.committed_state.expect("committed state"),
                        state.dirty_state.expect("dirty state"),
                        last_input,
                    ));
                }
                inner_state.error = None;
            }
            self.core.as_mut().run_loop();
            let mut inner_state = self.state.0.lock();
            if let Some(_) = inner_state.as_ref().expect("state").error {
                let state = inner_state.take().expect("state");
                return Err(anyhow::format_err!(
                    "fastforwarder: {}",
                    state.error.expect("error")
                ));
            }
        }
    }
}
