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

/// Captures the full shadow-side state for replay-mode seeking. The
/// playback session pairs this with the stepper's `ReplayCheckpoint` +
/// stepper-core mgba state so that loading a snapshot restores both
/// sides — without this, the shadow would still be at its pre-seek tick
/// and would feed misaligned packets after the seek.
#[derive(Clone)]
pub struct ShadowSnapshot {
    pub mgba_state: Box<mgba::state::State>,
    pub rng: rand_pcg::Mcg128Xsl64,
    pub round_state: RoundState,
}

use crate::input::{Input, PartialInput};

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
        custom_screen_tick_limit: Option<u32>,
    ) -> anyhow::Result<Self> {
        Self::new_from_sram(
            rom,
            &save.to_sram_dump(),
            hooks,
            match_type,
            is_offerer,
            local_player_index,
            rng,
            custom_screen_tick_limit,
        )
    }

    /// Build a shadow for replay-style reconstruction (playback, export,
    /// eval, the golden suite). Pulls remote sram + match_type +
    /// is_offerer + local_player_index from `replay`, seeds the RNG from
    /// `replay.rng_seed`, and advances it past the one-bool draw that
    /// [`crate::battle::Match::pick_local_player_index`] would have
    /// consumed during the live match — so the shadow's per-game
    /// RNG-handling traps stay in sync with the recorded run.
    ///
    /// Live PvP uses [`Shadow::new`] instead: there, the bool draw
    /// happens during `pick_local_player_index` itself, and the
    /// post-draw RNG is what gets passed in.
    pub fn new_for_replay(
        rom: &[u8],
        replay: &crate::replay::Replay,
        hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
    ) -> anyhow::Result<Self> {
        use rand::SeedableRng;
        let mut rng = rand_pcg::Mcg128Xsl64::from_seed(replay.rng_seed);
        let _ = rand::Rng::gen::<bool>(&mut rng);
        let match_type = (replay.metadata.match_type as u8, replay.metadata.match_subtype as u8);
        Self::new_from_sram(
            rom,
            &replay.remote_sram_dump(),
            hooks,
            match_type,
            replay.is_offerer,
            replay.local_player_index,
            rng,
            // Replay reconstruction never runs the chip-select timer — its
            // forced inputs (if any) are already baked into the recorded stream.
            None,
        )
    }

    /// Same as [`Shadow::new`] but takes the SRAM dump directly. Used by the
    /// replay-via-shadow playback path, where the remote-side save is
    /// stored as raw bytes inside the replay file rather than as a parsed
    /// Save object.
    pub fn new_from_sram(
        rom: &[u8],
        save_sram: &[u8],
        hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
        match_type: (u8, u8),
        is_offerer: bool,
        local_player_index: u8,
        rng: rand_pcg::Mcg128Xsl64,
        custom_screen_tick_limit: Option<u32>,
    ) -> anyhow::Result<Self> {
        let mut core = mgba::core::Core::new_gba("tango", &mgba::core::Options { ..Default::default() })?;

        core.as_mut().load_rom(mgba::vfile::VFile::from_vec(rom.to_vec()))?;
        core.as_mut()
            .load_save(mgba::vfile::VFile::from_vec(save_sram.to_vec()))?;

        let state = State::new(match_type, is_offerer, local_player_index, rng, custom_screen_tick_limit);

        hooks.patch(core.as_mut());

        let mut traps = hooks.common_traps();
        traps.extend(hooks.shadow_traps(state.clone()));
        core.set_traps(traps);
        core.as_mut().reset();
        // The shadow only derives the remote side's packets (game logic); its
        // pixels are never shown, so skip rasterization. Set after reset() (which
        // zeroes frameskip); it sticks, as frameskip isn't serialized.
        core.as_mut().gba_mut().set_frameskip(i32::MAX);

        Ok(Shadow { core, hooks, state })
    }

    pub fn save_state(&mut self) -> anyhow::Result<ShadowSnapshot> {
        let mgba_state = self.core.as_mut().save_state()?;
        let rng = self.state.0.rng.lock().unwrap().clone();
        let round_state = self.state.0.round_state.lock().unwrap().clone();
        Ok(ShadowSnapshot {
            mgba_state,
            rng,
            round_state,
        })
    }

    pub fn load_state(&mut self, snapshot: &ShadowSnapshot) -> anyhow::Result<()> {
        self.core.as_mut().load_state(&snapshot.mgba_state)?;
        *self.state.0.rng.lock().unwrap() = snapshot.rng.clone();
        *self.state.0.round_state.lock().unwrap() = snapshot.round_state.clone();
        // input_applied and error are per-run scratch; clear so the next
        // apply_input / round-end run doesn't pick up stale values that don't
        // correspond to the just-restored core state.
        self.state
            .0
            .input_applied
            .store(false, std::sync::atomic::Ordering::Relaxed);
        *self.state.0.error.lock().unwrap() = None;
        Ok(())
    }

    /// Run the shadow until the per-game traps mark this round's first
    /// committed state. `end_run_loop` parks the core right there, so there's
    /// nothing to load back — the next apply_input run continues from here.
    pub fn advance_until_first_committed_state(&mut self) -> anyhow::Result<()> {
        log::info!("advancing shadow until first committed state");
        loop {
            self.core.as_mut().run_loop();
            if let Some(err) = self.state.0.error.lock().unwrap().take() {
                return Err(anyhow::format_err!("shadow: {}", err));
            }

            let mut round_state = self.state.lock_round_state();
            let Some(round) = round_state.round.as_mut() else {
                continue;
            };
            if !round.has_first_committed_state() {
                continue;
            }
            round.current_tick = 0;
            return Ok(());
        }
    }

    /// Run the shadow until `end_round` drops the round state. `end_run_loop`
    /// in `round_end_entry` parks the core right at round end, so there's
    /// nothing to load back.
    pub fn advance_until_round_end(&mut self) -> anyhow::Result<()> {
        log::info!("advancing shadow until round end");
        self.hooks.prepare_for_fastforward(self.core.as_mut());
        loop {
            self.core.as_mut().run_loop();
            if let Some(err) = self.state.0.error.lock().unwrap().take() {
                return Err(anyhow::format_err!("shadow: {}", err));
            }
            if self.state.lock_round_state().round.is_none() {
                return Ok(());
            }
        }
    }

    /// Inject the given input pair as the next shadow input, then run the
    /// shadow forward one tick from wherever it is parked, until the per-game
    /// trap signals the input was applied: the core has reached the next tick's
    /// `main_read_joyflags`, where the trap calls `end_run_loop`, which parks the
    /// core exactly at that boundary. This call only ever advances; a rollback
    /// rewinds the shadow beforehand via [`load_state`](Self::load_state) (the
    /// rollback engine drives the primary and shadow cores in lockstep), so each
    /// `apply_input` resumes from the rewound position. `expected_tick` is
    /// unused, kept only to match the resolver callback signature. Returns the
    /// remote packet queued before this run.
    pub fn apply_input(&mut self, expected_tick: u32, ip: (Input, PartialInput)) -> anyhow::Result<Vec<u8>> {
        let pending_remote_packet = {
            let mut round_state = self.state.lock_round_state();
            let round = round_state.round.as_mut().expect("round");
            round.set_pending_shadow_input(ip);
            round.peek_remote_packet().expect("pending remote packet")
        };
        // Discard any stale "input applied" signal before this run. The per-game
        // trap sets it whenever `take_input_injected()` fires, which also happens
        // outside apply_input — e.g. while `advance_until_round_end` runs the game
        // through round-end link-cable exchanges. The old shared `applied_snapshot`
        // signal was cleared by whichever of apply_input / advance_until_round_end
        // `.take()`'d it; the split into `input_applied` lost that, so a leftover
        // `true` would make the next round's first apply_input return before it
        // actually applied its input.
        self.state.take_input_applied();
        self.hooks.prepare_for_fastforward(self.core.as_mut());
        loop {
            self.core.as_mut().run_loop();
            if let Some(err) = self.state.0.error.lock().unwrap().take() {
                return Err(anyhow::format_err!("shadow: {}", err));
            }
            if !self.state.take_input_applied() {
                continue;
            }
            let _ = expected_tick;
            return Ok(pending_remote_packet);
        }
    }
}
