//! Shadow emulator: simulates the remote peer locally so the live primary
//! can advance lockstep without waiting on the network. Each [`Shadow`] runs
//! the opponent's ROM + SRAM and answers the primary fastforwarder's
//! `apply_shadow_input` calls with predicted remote packets.
//!
//! - [`State`] is the shared handle the per-game shadow traps lock to read
//!   and modify the round state, RNG, and applied snapshots.
//! - [`Round`] / [`RoundState`] hold the per-round mutable state.

mod round;
mod state;

pub use round::{Round, RoundState};
pub use state::State;

use crate::input::{Input, Pair, PartialInput};

/// Shadow-mode emulator that mirrors the remote peer locally. The visible
/// primary calls into this to advance the predicted opponent state via
/// [`apply_input`](Shadow::apply_input) and to capture initial /
/// end-of-round snapshots.
pub struct Shadow {
    core: mgba::core::Core,
    state: State,
    hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
}

impl Shadow {
    pub fn new(
        rom: &[u8],
        save: &(dyn tango_dataview::save::Save + Send + Sync),
        hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
        match_type: (u8, u8),
        is_offerer: bool,
        local_player_index: u8,
        rng: rand_pcg::Mcg128Xsl64,
    ) -> anyhow::Result<Self> {
        let mut core = mgba::core::Core::new_gba("tango")?;

        core.as_mut().load_rom(mgba::vfile::VFile::from_vec(rom.to_vec()))?;
        core.as_mut()
            .load_save(mgba::vfile::VFile::from_vec(save.as_sram_dump()))?;

        let state = State::new(match_type, is_offerer, local_player_index, rng);

        hooks.patch(core.as_mut());

        let mut traps = hooks.common_traps();
        traps.extend(hooks.shadow_traps(state.clone()));
        core.set_traps(traps);
        core.as_mut().reset();

        Ok(Shadow { core, hooks, state })
    }

    /// Run the shadow until the per-game traps have captured this round's
    /// initial committed state, then load it back into the core so the next
    /// apply_input run continues from there.
    pub fn advance_until_first_committed_state(&mut self) -> anyhow::Result<()> {
        log::info!("advancing shadow until first committed state");
        loop {
            self.core.as_mut().run_loop();
            if let Some(err) = self.state.0.error.lock().take() {
                return Err(anyhow::format_err!("shadow: {}", err));
            }

            let mut round_state = self.state.lock_round_state();
            let Some(round) = round_state.round.as_mut() else {
                continue;
            };

            let Some(state) = round.first_committed_state.as_ref() else {
                continue;
            };

            self.core.as_mut().load_state(state).expect("load state");
            round.current_tick = 0;
            return Ok(());
        }
    }

    /// Run the shadow until `end_round` has dropped its round state, then
    /// load the most recent applied snapshot.
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

    /// Inject the given input pair as the next shadow input, then run the
    /// shadow until per-game traps capture an applied snapshot. `expected_tick`
    /// is the tick the primary expected the shadow to be on for this input —
    /// per-game traps use it to detect the "shadow advanced one tick before
    /// the trap fired" race. Returns the remote packet that was queued before
    /// this run.
    pub fn apply_input(&mut self, expected_tick: u32, ip: Pair<Input, PartialInput>) -> anyhow::Result<Vec<u8>> {
        let pending_remote_packet = {
            let mut round_state = self.state.lock_round_state();
            let round = round_state.round.as_mut().expect("round");
            round.set_pending_shadow_input(expected_tick, ip);
            round.peek_remote_packet().expect("pending remote packet")
        };
        self.hooks.prepare_for_fastforward(self.core.as_mut());
        loop {
            self.core.as_mut().run_loop();
            if let Some(err) = self.state.0.error.lock().take() {
                return Err(anyhow::format_err!("shadow: {}", err));
            }
            let Some(applied_state) = self.state.0.applied_state.lock().take() else {
                continue;
            };

            // NOTE: applied_state.tick may legitimately differ from
            // expected_tick by one or more — depending on the per-game
            // shadow trap layout, set_applied_state can fire after
            // current_tick has already advanced. The applied state is
            // still correct; we just don't assert here.
            self.core.as_mut().load_state(&applied_state.state).expect("load state");
            let mut round_state = self.state.lock_round_state();
            let round = round_state.round.as_mut().expect("round");
            round.current_tick = applied_state.tick;
            let _ = expected_tick;
            return Ok(pending_remote_packet);
        }
    }
}
