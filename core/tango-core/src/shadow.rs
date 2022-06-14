use crate::{hooks, input};

pub struct Round {
    current_tick: u32,
    local_player_index: u8,
    first_committed_state: Option<mgba::state::State>,
    pending_in_input: Option<input::Pair<input::Input, input::PartialInput>>,
    pending_out_input: Option<input::Pair<input::Input, input::Input>>,
    input_injected: bool,
}

impl Round {
    pub fn current_tick(&self) -> u32 {
        self.current_tick
    }

    pub fn increment_current_tick(&mut self) {
        self.current_tick += 1;
    }

    pub fn local_player_index(&self) -> u8 {
        self.local_player_index
    }

    pub fn remote_player_index(&self) -> u8 {
        1 - self.local_player_index
    }

    pub fn set_first_committed_state(&mut self, state: mgba::state::State) {
        self.first_committed_state = Some(state);
    }

    pub fn has_first_committed_state(&self) -> bool {
        self.first_committed_state.is_some()
    }

    pub fn take_in_input_pair(&mut self) -> Option<input::Pair<input::Input, input::PartialInput>> {
        self.pending_in_input.take()
    }

    pub fn set_out_input_pair(&mut self, ip: input::Pair<input::Input, input::Input>) {
        self.pending_out_input = Some(ip);
    }

    pub fn peek_in_input_pair(
        &mut self,
    ) -> &Option<input::Pair<input::Input, input::PartialInput>> {
        &self.pending_in_input
    }

    pub fn peek_out_input_pair(&self) -> &Option<input::Pair<input::Input, input::Input>> {
        &self.pending_out_input
    }

    pub fn set_input_injected(&mut self) {
        self.input_injected = true;
    }

    pub fn take_input_injected(&mut self) -> bool {
        std::mem::replace(&mut self.input_injected, false)
    }
}

pub struct RoundState {
    pub round: Option<Round>,
    pub won_last_round: Option<bool>,
}

struct AppliedState {
    tick: u32,
    state: mgba::state::State,
}

struct InnerState {
    match_type: u8,
    is_offerer: bool,
    round_state: parking_lot::Mutex<RoundState>,
    rng: parking_lot::Mutex<rand_pcg::Mcg128Xsl64>,
    applied_state: parking_lot::Mutex<Option<AppliedState>>,
    error: parking_lot::Mutex<Option<anyhow::Error>>,
}

pub struct Shadow {
    core: mgba::core::Core,
    state: State,
    hooks: &'static Box<dyn hooks::Hooks + Send + Sync>,
}

#[derive(Clone)]
pub struct State(std::sync::Arc<InnerState>);

impl State {
    pub fn new(
        match_type: u8,
        is_offerer: bool,
        rng: rand_pcg::Mcg128Xsl64,
        won_last_round: bool,
    ) -> State {
        State(std::sync::Arc::new(InnerState {
            match_type,
            is_offerer,
            rng: parking_lot::Mutex::new(rng),
            round_state: parking_lot::Mutex::new(RoundState {
                round: None,
                won_last_round: Some(won_last_round),
            }),
            applied_state: parking_lot::Mutex::new(None),
            error: parking_lot::Mutex::new(None),
        }))
    }

    pub fn match_type(&self) -> u8 {
        self.0.match_type
    }

    pub fn is_offerer(&self) -> bool {
        self.0.is_offerer
    }

    pub fn lock_rng(&self) -> parking_lot::MutexGuard<rand_pcg::Mcg128Xsl64> {
        self.0.rng.lock()
    }

    pub fn lock_round_state(&self) -> parking_lot::MutexGuard<'_, RoundState> {
        self.0.round_state.lock()
    }

    pub fn start_round(&self) {
        let mut round_state = self.0.round_state.lock();
        round_state.round = Some(Round {
            current_tick: 0,
            local_player_index: if round_state.won_last_round.take().unwrap() {
                0
            } else {
                1
            },
            first_committed_state: None,
            pending_in_input: None,
            pending_out_input: None,
            input_injected: false,
        });
    }

    pub fn end_round(&self) {
        log::info!("shadow round ended");
        let mut round_state = self.0.round_state.lock();
        round_state.round = None;
    }

    pub fn set_won_last_round(&self, did_win: bool) {
        self.0.round_state.lock().won_last_round = Some(did_win);
    }

    pub fn set_anyhow_error(&self, err: anyhow::Error) {
        *self.0.error.lock() = Some(err);
    }

    pub fn set_applied_state(&self, state: mgba::state::State, tick: u32) {
        *self.0.applied_state.lock() = Some(AppliedState { tick, state });
    }
}

impl Shadow {
    pub fn new(
        rom: &[u8],
        save_path: &std::path::Path,
        match_type: u8,
        is_offerer: bool,
        won_last_round: bool,
        rng: rand_pcg::Mcg128Xsl64,
    ) -> anyhow::Result<Self> {
        let mut core = mgba::core::Core::new_gba("tango")?;
        let rom_vf = mgba::vfile::VFile::open_memory(rom);
        core.as_mut().load_rom(rom_vf)?;

        log::info!(
            "loaded shadow game: {} rev {}",
            std::str::from_utf8(&core.as_mut().full_rom_name()).unwrap(),
            core.as_mut().rom_revision(),
        );

        let save_vf = mgba::vfile::VFile::open(
            save_path,
            mgba::vfile::flags::O_CREAT | mgba::vfile::flags::O_RDWR,
        )?;
        core.as_mut().load_save(save_vf)?;

        let state = State::new(match_type, is_offerer, rng, won_last_round);

        let hooks = hooks::get(core.as_mut()).unwrap();
        hooks.patch(core.as_mut());

        let mut traps = hooks.common_traps();
        traps.extend(hooks.shadow_traps(state.clone()));
        core.set_traps(traps);
        core.as_mut().reset();

        Ok(Shadow { core, hooks, state })
    }

    pub fn advance_until_first_committed_state(&mut self) -> anyhow::Result<mgba::state::State> {
        log::info!("advancing shadow until first committed state");
        loop {
            self.core.as_mut().run_loop();
            if let Some(err) = self.state.0.error.lock().take() {
                return Err(anyhow::format_err!("shadow: {}", err));
            }

            let mut round_state = self.state.lock_round_state();
            let round = if let Some(round) = round_state.round.as_mut() {
                round
            } else {
                continue;
            };

            let state = if let Some(state) = round.first_committed_state.as_ref() {
                state.clone()
            } else {
                continue;
            };

            self.core.as_mut().load_state(&state).expect("load state");
            round.current_tick = 0;
            return Ok(state);
        }
    }

    pub fn advance_until_round_end(&mut self) -> anyhow::Result<()> {
        log::info!("advancing shadow until round end");
        self.hooks.prepare_for_fastforward(self.core.as_mut());
        loop {
            self.core.as_mut().run_loop();
            if let Some(err) = self.state.0.error.lock().take() {
                return Err(anyhow::format_err!("shadow: {}", err));
            }

            let round_state = self.state.lock_round_state();
            if round_state.round.is_none() {
                let applied_state = self
                    .state
                    .0
                    .applied_state
                    .lock()
                    .take()
                    .expect("applied state");

                self.core
                    .as_mut()
                    .load_state(&applied_state.state)
                    .expect("load state");
                return Ok(());
            }
        }
    }

    pub fn apply_input(
        &mut self,
        input: input::Pair<input::Input, input::PartialInput>,
    ) -> anyhow::Result<input::Pair<input::Input, input::Input>> {
        {
            let mut round_state = self.state.lock_round_state();
            let round = round_state.round.as_mut().expect("round");
            round.pending_in_input = Some(input);
        }
        self.hooks.prepare_for_fastforward(self.core.as_mut());
        loop {
            self.core.as_mut().run_loop();
            if let Some(err) = self.state.0.error.lock().take() {
                return Err(anyhow::format_err!("shadow: {}", err));
            }
            let applied_state =
                if let Some(applied_state) = self.state.0.applied_state.lock().take() {
                    applied_state
                } else {
                    continue;
                };

            self.core
                .as_mut()
                .load_state(&applied_state.state)
                .expect("load state");
            let mut round_state = self.state.lock_round_state();
            let round = round_state.round.as_mut().expect("round");
            round.current_tick = applied_state.tick;
            return Ok(round.pending_out_input.take().expect("pending out input"));
        }
    }
}
