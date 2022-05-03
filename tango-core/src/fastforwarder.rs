use crate::hooks;
use crate::input;

struct InnerState {
    local_player_index: u8,
    input_pairs: std::collections::VecDeque<input::Pair<input::Input, input::Input>>,
    commit_time: u32,
    committed_state: Option<mgba::state::State>,
    dirty_time: u32,
    dirty_state: Option<mgba::state::State>,
    on_battle_ended: Box<dyn Fn() + Send>,
    error: Option<anyhow::Error>,
}

impl InnerState {
    pub fn new(
        local_player_index: u8,
        input_pairs: Vec<input::Pair<input::Input, input::Input>>,
        commit_time: u32,
        dirty_time: u32,
        on_battle_ended: Box<dyn Fn() + Send>,
    ) -> Self {
        InnerState {
            local_player_index,
            input_pairs: input_pairs.into_iter().collect(),
            commit_time,
            committed_state: None,
            dirty_time,
            dirty_state: None,
            on_battle_ended,
            error: None,
        }
    }
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
        dirty_time: u32,
        on_battle_ended: Box<dyn Fn() + Send>,
    ) -> State {
        State(std::sync::Arc::new(parking_lot::Mutex::new(Some(
            InnerState::new(
                local_player_index,
                input_pairs,
                commit_time,
                dirty_time,
                on_battle_ended,
            ),
        ))))
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
}

impl Fastforwarder {
    pub fn new(
        rom_path: &std::path::Path,
        hooks: &'static Box<dyn hooks::Hooks + Send + Sync>,
        local_player_index: u8,
    ) -> anyhow::Result<Self> {
        let mut core = mgba::core::Core::new_gba("tango")?;
        let rom_vf = mgba::vfile::VFile::open(rom_path, mgba::vfile::flags::O_RDONLY)?;
        core.as_mut().load_rom(rom_vf)?;

        let state = State(std::sync::Arc::new(parking_lot::Mutex::new(None)));

        core.set_traps(hooks.fastforwarder_traps(state.clone()));
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
        commit_pairs: &[input::Pair<input::Input, input::Input>],
        last_committed_remote_input: input::Input,
        local_player_inputs_left: &[input::Input],
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
                        custom_screen_state: last_committed_remote_input.custom_screen_state,
                        turn: vec![],
                    },
                }
            }))
            .collect::<Vec<input::Pair<input::Input, input::Input>>>();
        let last_input = input_pairs.last().expect("last input pair").clone();

        self.core.as_mut().load_state(state)?;
        self.hooks.prepare_for_fastforward(self.core.as_mut());

        let start_current_tick = self.hooks.current_tick(self.core.as_mut());
        let commit_time = start_current_tick + commit_pairs.len() as u32;
        let dirty_time = start_current_tick + input_pairs.len() as u32 - 1;

        *self.state.0.lock() = Some(InnerState::new(
            self.local_player_index,
            input_pairs,
            commit_time,
            dirty_time,
            Box::new(|| {}),
        ));

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
                return Err(state.error.expect("error"));
            }
        }
    }
}
