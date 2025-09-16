pub struct Round {
    current_tick: u32,
    local_player_index: u8,
    first_committed_state: Option<Box<mgba::state::State>>,
    pending_shadow_input: Option<crate::input::Pair<crate::input::Input, crate::input::PartialInput>>,
    pending_remote_packet: Option<crate::input::Packet>,
    input_injected: bool,
}

impl Round {
    pub fn on_draw_outcome(&self) -> crate::battle::BattleOutcome {
        match self.local_player_index {
            0 => crate::battle::BattleOutcome::Win,
            1 => crate::battle::BattleOutcome::Loss,
            _ => unreachable!(),
        }
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

    pub fn remote_player_index(&self) -> u8 {
        1 - self.local_player_index
    }

    pub fn set_first_committed_state(&mut self, state: Box<mgba::state::State>, packet: &[u8]) {
        self.first_committed_state = Some(state);
        self.pending_remote_packet = Some(crate::input::Packet {
            tick: 0,
            packet: packet.to_vec(),
        });
    }

    pub fn has_first_committed_state(&self) -> bool {
        self.first_committed_state.is_some()
    }

    pub fn take_shadow_input(&mut self) -> Option<crate::input::Pair<crate::input::Input, crate::input::PartialInput>> {
        self.pending_shadow_input.take()
    }

    pub fn set_remote_packet(&mut self, tick: u32, packet: Vec<u8>) {
        self.pending_remote_packet = Some(crate::input::Packet { tick, packet });
    }

    pub fn peek_remote_packet(&self) -> Option<crate::input::Packet> {
        self.pending_remote_packet.clone()
    }

    pub fn peek_shadow_input(
        &mut self,
    ) -> &Option<crate::input::Pair<crate::input::Input, crate::input::PartialInput>> {
        &self.pending_shadow_input
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
    pub last_outcome: Option<crate::battle::BattleOutcome>,
}

impl RoundState {
    pub fn set_last_outcome(&mut self, last_outcome: crate::battle::BattleOutcome) {
        self.last_outcome = Some(last_outcome);
    }
}

struct AppliedState {
    tick: u32,
    state: Box<mgba::state::State>,
}

struct InnerState {
    match_type: (u8, u8),
    is_offerer: bool,
    round_state: parking_lot::Mutex<RoundState>,
    rng: parking_lot::Mutex<rand_pcg::Mcg128Xsl64>,
    applied_state: parking_lot::Mutex<Option<AppliedState>>,
    error: parking_lot::Mutex<Option<anyhow::Error>>,
}

pub struct Shadow {
    core: mgba::core::Core,
    state: State,
    hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
}

#[derive(Clone)]
pub struct State(std::sync::Arc<InnerState>);

impl State {
    pub fn new(
        match_type: (u8, u8),
        is_offerer: bool,
        rng: rand_pcg::Mcg128Xsl64,
        last_outcome: crate::battle::BattleOutcome,
    ) -> State {
        State(std::sync::Arc::new(InnerState {
            match_type,
            is_offerer,
            rng: parking_lot::Mutex::new(rng),
            round_state: parking_lot::Mutex::new(RoundState {
                round: None,
                last_outcome: Some(last_outcome),
            }),
            applied_state: parking_lot::Mutex::new(None),
            error: parking_lot::Mutex::new(None),
        }))
    }

    pub fn match_type(&self) -> (u8, u8) {
        self.0.match_type
    }

    pub fn is_offerer(&self) -> bool {
        self.0.is_offerer
    }

    pub fn lock_rng(&self) -> parking_lot::MutexGuard<'_, rand_pcg::Mcg128Xsl64> {
        self.0.rng.lock()
    }

    pub fn lock_round_state(&self) -> parking_lot::MutexGuard<'_, RoundState> {
        self.0.round_state.lock()
    }

    pub fn start_round(&self) {
        let mut round_state = self.0.round_state.lock();
        let local_player_index = match round_state.last_outcome.take().unwrap() {
            crate::battle::BattleOutcome::Win => 0,
            crate::battle::BattleOutcome::Loss => 1,
        };
        log::info!("starting shadow round: local_player_index = {}", local_player_index);
        round_state.round = Some(Round {
            current_tick: 0,
            local_player_index,
            first_committed_state: None,
            pending_shadow_input: None,
            pending_remote_packet: None,
            input_injected: false,
        });
    }

    pub fn end_round(&self) {
        log::info!("shadow round ended");
        let mut round_state = self.0.round_state.lock();
        round_state.round = None;
    }

    pub fn set_anyhow_error(&self, err: anyhow::Error) {
        *self.0.error.lock() = Some(err);
    }

    pub fn set_applied_state(&self, state: Box<mgba::state::State>, tick: u32) {
        *self.0.applied_state.lock() = Some(AppliedState { tick, state });
    }
}

impl Shadow {
    pub fn new(
        rom: &[u8],
        save: &(dyn tango_dataview::save::Save + Send + Sync),
        hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
        match_type: (u8, u8),
        is_offerer: bool,
        battle_result: crate::battle::BattleOutcome,
        rng: rand_pcg::Mcg128Xsl64,
    ) -> anyhow::Result<Self> {
        let mut core = mgba::core::Core::new_gba("tango")?;

        core.as_mut().load_rom(mgba::vfile::VFile::from_vec(rom.to_vec()))?;
        core.as_mut()
            .load_save(mgba::vfile::VFile::from_vec(save.as_sram_dump()))?;

        let state = State::new(match_type, is_offerer, rng, battle_result);

        hooks.patch(core.as_mut());

        let mut traps = hooks.common_traps();
        traps.extend(hooks.shadow_traps(state.clone()));
        core.set_traps(traps);
        core.as_mut().reset();

        Ok(Shadow { core, hooks, state })
    }

    pub fn advance_until_first_committed_state(&mut self) -> anyhow::Result<Box<mgba::state::State>> {
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
                let applied_state = self.state.0.applied_state.lock().take().expect("applied state");

                self.core.as_mut().load_state(&applied_state.state).expect("load state");
                return Ok(());
            }
        }
    }

    pub fn apply_input(
        &mut self,
        ip: crate::input::Pair<crate::input::Input, crate::input::PartialInput>,
    ) -> anyhow::Result<crate::input::Packet> {
        let pending_remote_packet = {
            let mut round_state = self.state.lock_round_state();
            let round = round_state.round.as_mut().expect("round");
            round.pending_shadow_input = Some(ip);
            round.pending_remote_packet.clone().expect("pending remote packet")
        };
        self.hooks.prepare_for_fastforward(self.core.as_mut());
        loop {
            self.core.as_mut().run_loop();
            if let Some(err) = self.state.0.error.lock().take() {
                return Err(anyhow::format_err!("shadow: {}", err));
            }
            let applied_state = if let Some(applied_state) = self.state.0.applied_state.lock().take() {
                applied_state
            } else {
                continue;
            };

            self.core.as_mut().load_state(&applied_state.state).expect("load state");
            let mut round_state = self.state.lock_round_state();
            let round = round_state.round.as_mut().expect("round");
            round.current_tick = applied_state.tick;
            return Ok(pending_remote_packet);
        }
    }
}
