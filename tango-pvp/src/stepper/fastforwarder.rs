use crate::input::{Input, Pair, PartialInput};

use super::state::{InnerState, State};
use super::types::RoundResult;

/// Output of a single Fastforwarder run.
pub struct FastforwardResult {
    pub committed_state: crate::battle::CommittedState,
    /// State to hand the display core: the run's estimate of
    /// `frontier - presentation_delay`, captured at the same `main_read_joyflags`
    /// point as `dirty_state` so it loads seamlessly on the render core.
    /// `None` when no display core is active (`present_tick` left past
    /// `dirty_tick`), so games on the legacy single-core path pay nothing.
    pub present_state: Option<Box<mgba::state::State>>,
    pub dirty_state: crate::battle::CommittedState,
    pub round_result: Option<RoundResult>,
    pub output_pairs: Vec<Pair<Input, Input>>,
}

/// Per-Match emulator dedicated to running the per-frame stepper traps over a
/// known input window. Each [`fastforward`](Fastforwarder::fastforward) call
/// loads a saved state, processes the input pairs, and returns fresh
/// committed and dirty save snapshots.
pub struct Fastforwarder {
    core: mgba::core::Core,
    state: State,
    hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
    match_type: (u8, u8),
    local_player_index: u8,
}

impl Fastforwarder {
    pub fn new(
        rom: &[u8],
        hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
        match_type: (u8, u8),
        local_player_index: u8,
    ) -> anyhow::Result<Self> {
        let mut core = mgba::core::Core::new_gba("tango")?;
        let rom_vf = mgba::vfile::VFile::from_vec(rom.to_vec());
        core.as_mut().load_rom(rom_vf)?;
        hooks.patch(core.as_mut());

        let state = State(std::sync::Arc::new(parking_lot::Mutex::new(None)));

        let mut traps = hooks.common_traps();
        traps.extend(hooks.stepper_traps(state.clone()));
        core.set_traps(traps);
        core.as_mut().reset();
        // Headless re-sim core: never rasterize. Its pixels are never shown (the
        // display core re-renders from the states this captures), and it re-sims
        // the speculative window every frame, so skipping drawScanline cuts a
        // large constant off the dominant cost. Set after reset() — which zeroes
        // frameskip — and it sticks (frameskip isn't serialized).
        core.as_mut().gba_mut().set_frameskip(i32::MAX);

        Ok(Fastforwarder {
            core,
            state,
            hooks,
            match_type,
            local_player_index,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn fastforward(
        &mut self,
        state: &mgba::state::State,
        input_pairs: Vec<Pair<PartialInput, PartialInput>>,
        current_tick: u32,
        commit_tick: u32,
        dirty_tick: u32,
        present_tick: u32,
        last_local_packet: &[u8],
        apply_shadow_input: Box<dyn FnMut(u32, Pair<Input, PartialInput>) -> anyhow::Result<Vec<u8>> + Sync + Send>,
    ) -> anyhow::Result<FastforwardResult> {
        self.core.as_mut().load_state(state)?;
        self.hooks.prepare_for_fastforward(self.core.as_mut());

        *self.state.0.lock() = Some(InnerState::for_fastforward(
            self.match_type,
            self.local_player_index,
            input_pairs,
            current_tick,
            commit_tick,
            dirty_tick,
            present_tick,
            last_local_packet.to_vec(),
            apply_shadow_input,
        ));

        loop {
            {
                let mut guard = self.state.0.lock();
                let inner = guard.as_mut().unwrap();
                if inner.has_committed_state_snapshot()
                    && inner.has_dirty_state_snapshot()
                    && inner.has_present_state_snapshot()
                {
                    return Ok(guard.take().expect("state").into_fastforward_result());
                }
                let _ = inner.take_error();
            }
            self.core.as_mut().run_loop();
            let mut guard = self.state.0.lock();
            if let Some(err) = guard.as_mut().expect("state").take_error() {
                guard.take();
                return Err(anyhow::format_err!("replayer: {}", err));
            }
        }
    }
}
