use crate::{hooks, input};

struct PendingTurn {
    tx_buf: Vec<u8>,
    on_tick: u32,
}

pub struct Round {
    local_player_index: u8,
    is_accepting_input: bool,
    first_committed_state: Option<mgba::state::State>,
    pending_out_turn: Option<PendingTurn>,
    pending_in_input: Option<input::Pair<input::Input, input::PartialInput>>,
    pending_out_input: Option<input::Pair<input::Input, input::Input>>,
    input_injected: bool,
}

impl Round {
    pub fn local_player_index(&self) -> u8 {
        self.local_player_index
    }

    pub fn remote_player_index(&self) -> u8 {
        1 - self.local_player_index
    }

    pub fn start_accepting_input(&mut self) {
        self.is_accepting_input = true;
    }

    pub fn is_accepting_input(&mut self) -> bool {
        self.is_accepting_input
    }

    pub fn set_first_committed_state(&mut self, state: mgba::state::State) {
        log::info!("shadow state committed");
        self.first_committed_state = Some(state);
    }

    pub fn has_first_committed_state(&self) -> bool {
        self.first_committed_state.is_some()
    }

    pub fn set_pending_out_turn(&mut self, tx_buf: Vec<u8>, on_tick: u32) {
        self.pending_out_turn = Some(PendingTurn { on_tick, tx_buf });
    }

    pub fn take_pending_out_turn(&mut self, current_tick: u32) -> Vec<u8> {
        match &mut self.pending_out_turn {
            Some(pt) => {
                if pt.on_tick == current_tick {
                    self.pending_out_turn.take().unwrap().tx_buf
                } else {
                    vec![]
                }
            }
            None => vec![],
        }
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
        let input_injected = self.input_injected;
        self.input_injected = false;
        input_injected
    }
}

pub struct RoundState {
    pub round: Option<Round>,
    pub won_last_round: bool,
}

struct InnerState {
    match_type: u16,
    is_offerer: bool,
    round_state: parking_lot::Mutex<RoundState>,
    rng: parking_lot::Mutex<rand_pcg::Mcg128Xsl64>,
    pending_in_init: parking_lot::Mutex<Option<Vec<u8>>>,
    pending_out_init: parking_lot::Mutex<Option<Vec<u8>>>,
    applied_state: parking_lot::Mutex<Option<mgba::state::State>>,
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
        match_type: u16,
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
                won_last_round,
            }),
            pending_in_init: parking_lot::Mutex::new(None),
            pending_out_init: parking_lot::Mutex::new(None),
            applied_state: parking_lot::Mutex::new(None),
            error: parking_lot::Mutex::new(None),
        }))
    }

    pub fn match_type(&self) -> u16 {
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

    pub fn set_pending_out_init(&self, tx_buf: Vec<u8>) {
        *self.0.pending_out_init.lock() = Some(tx_buf);
    }

    pub fn take_pending_in_init(&self) -> Option<Vec<u8>> {
        self.0.pending_in_init.lock().take()
    }

    pub fn start_round(&self) {
        let mut round_state = self.0.round_state.lock();
        round_state.round = Some(Round {
            local_player_index: if round_state.won_last_round { 0 } else { 1 },
            is_accepting_input: false,
            first_committed_state: None,
            pending_out_turn: None,
            pending_in_input: None,
            pending_out_input: None,
            input_injected: false,
        });
    }

    pub fn end_round(&self) {
        let mut round_state = self.0.round_state.lock();
        round_state.round = None;
    }

    pub fn set_won_last_round(&self, did_win: bool) {
        self.0.round_state.lock().won_last_round = did_win;
    }

    pub fn set_anyhow_error(&self, err: anyhow::Error) {
        *self.0.error.lock() = Some(err);
    }

    pub fn set_applied_state(&self, state: mgba::state::State) {
        *self.0.applied_state.lock() = Some(state);
    }
}

impl Shadow {
    pub fn new(
        rom_path: &std::path::Path,
        save_path: &std::path::Path,
        hooks: &'static Box<dyn hooks::Hooks + Send + Sync>,
        match_type: u16,
        is_offerer: bool,
        won_last_round: bool,
        rng: rand_pcg::Mcg128Xsl64,
    ) -> anyhow::Result<Self> {
        let mut core = mgba::core::Core::new_gba("tango")?;
        let rom_vf = mgba::vfile::VFile::open(rom_path, mgba::vfile::flags::O_RDONLY)?;
        core.as_mut().load_rom(rom_vf)?;

        log::info!("loaded shadow game: {}", core.as_ref().game_title());

        let save_vf = mgba::vfile::VFile::open(
            &save_path,
            mgba::vfile::flags::O_CREAT | mgba::vfile::flags::O_RDWR,
        )?;
        core.as_mut().load_save(save_vf)?;

        let state = State::new(match_type, is_offerer, rng, won_last_round);

        core.set_traps(hooks.shadow_traps(state.clone()));
        core.as_mut().reset();

        Ok(Shadow { core, hooks, state })
    }

    pub fn exchange_init(&mut self, local_init: Vec<u8>) -> anyhow::Result<Vec<u8>> {
        log::info!("exchanging init with shadow");
        *self.state.0.pending_in_init.lock() = Some(local_init);

        loop {
            self.core.as_mut().run_loop();
            if let Some(err) = self.state.0.error.lock().take() {
                return Err(err.into());
            }
            if let Some(buf) = self.state.0.pending_out_init.lock().take() {
                log::info!("init exchanged!");
                self.core
                    .as_mut()
                    .load_state(
                        &self
                            .state
                            .0
                            .applied_state
                            .lock()
                            .take()
                            .expect("applied state"),
                    )
                    .expect("load state");
                return Ok(buf);
            }
        }
    }

    pub fn advance_until_first_committed_state(&mut self) -> anyhow::Result<mgba::state::State> {
        log::info!("advancing shadow until first committed state");
        loop {
            self.core.as_mut().run_loop();
            if let Some(err) = self.state.0.error.lock().take() {
                return Err(err.into());
            }

            let round_state = self.state.lock_round_state();
            if let Some(state) = round_state
                .round
                .as_ref()
                .and_then(|round| round.first_committed_state.as_ref())
            {
                self.core.as_mut().load_state(&state).expect("load state");
                log::info!("advanced to committed state!");
                return Ok(state.clone());
            }
        }
    }

    pub fn advance_until_round_end(&mut self) -> anyhow::Result<()> {
        log::info!("advancing shadow until round end");
        self.hooks.prepare_for_fastforward(self.core.as_mut());
        loop {
            self.core.as_mut().run_loop();
            if let Some(err) = self.state.0.error.lock().take() {
                return Err(err.into());
            }

            let round_state = self.state.lock_round_state();
            if round_state.round.is_none() {
                self.core
                    .as_mut()
                    .load_state(
                        &self
                            .state
                            .0
                            .applied_state
                            .lock()
                            .take()
                            .expect("applied state"),
                    )
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
                return Err(err.into());
            }
            if let Some(applied_state) = self.state.0.applied_state.lock().take() {
                self.core
                    .as_mut()
                    .load_state(&applied_state)
                    .expect("load state");
                let mut round_state = self.state.lock_round_state();
                let round = round_state.round.as_mut().expect("round");
                return Ok(round.pending_out_input.take().expect("pending out input"));
            }
        }
    }
}
