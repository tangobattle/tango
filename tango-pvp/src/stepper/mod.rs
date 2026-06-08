pub use crate::input::{Input, PartialInput};

mod state;

pub use state::{InnerState, ReplayCheckpoint, ReplaySnapshot, State};

/// Outcome of a single round, as detected by the per-game `round_end_*` traps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde_repr::Serialize_repr)]
#[repr(i8)]
pub enum BattleOutcome {
    Draw = -1,
    Loss = 0,
    Win = 1,
}

/// Phase tracking for the current round. Replay-mode round transitions and
/// the per-game `is_round_ending` gates flip through these.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum RoundPhase {
    InProgress,
    Ending,
    Ended,
}

/// Outcome bundled with the tick at which the GAME signaled it.
#[derive(Clone, Copy)]
pub struct RoundResult {
    pub tick: u32,
    pub outcome: BattleOutcome,
}

/// Output of a single stepper run.
pub struct StepperResult {
    /// State captured at `capture_tick`: the per-game stepper trap fires
    /// `main_read_joyflags` once the input window is exhausted and snapshots
    /// poised at the start of that tick, with r4 (local joyflags) left unset.
    /// The consumer supplies it: the live core via
    /// [`Hooks::inject_joyflags_on_primary_snapshot`](crate::hooks::Hooks::inject_joyflags_on_primary_snapshot)
    /// after loading the snapshot, and the next `fastforward` run by re-priming
    /// r4 at its first `main_read_joyflags` (its PC is rewound there by
    /// `prepare_for_fastforward`).
    pub snapshot: crate::battle::Snapshot,
    pub round_result: Option<RoundResult>,
    pub output_pairs: Vec<(Input, Input)>,
}

/// Single per-frame re-sim core for the rollback engine: a dedicated headless
/// mgba core plus the drive loop that runs the per-game trap set one tick at a
/// time, capturing a boundary snapshot at `capture_tick` each tick. It advances
/// via [`step`](Self::step) and rewinds via [`restore`](Self::restore) only when
/// the engine rolls back to re-simulate a mispredicted tail. In steady state —
/// and after promotions — `step` resumes forward from wherever the previous call
/// parked the core (`end_run_loop` halts exactly at the boundary
/// `main_read_joyflags` with r4 unset; see e.g. `game/bn6/stepper.rs`), so no
/// reload happens. Folds together what used to be two cores (a reload-each-frame
/// speculative fork and a forward-only authoritative core).
pub struct Stepper {
    core: mgba::core::Core,
    state: State,
    hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
    match_type: (u8, u8),
    local_player_index: u8,
}

impl Stepper {
    /// Build the stepper seeded at the round's first-committed state. The core
    /// is then parked at that boundary, ready for the first [`step`](Self::step).
    pub fn new(
        rom: &[u8],
        hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
        match_type: (u8, u8),
        local_player_index: u8,
        initial_state: &mgba::state::State,
    ) -> anyhow::Result<Self> {
        let mut core = mgba::core::Core::new_gba("tango", &mgba::core::Options { ..Default::default() })?;
        let rom_vf = mgba::vfile::VFile::from_vec(rom.to_vec());
        core.as_mut().load_rom(rom_vf)?;
        hooks.patch(core.as_mut());

        let state = State(std::sync::Arc::new(std::sync::Mutex::new(None)));

        let mut traps = hooks.common_traps();
        traps.extend(hooks.stepper_traps(state.clone()));
        core.set_traps(traps);
        core.as_mut().reset();
        // Headless re-sim core: never rasterize. Its pixels are never shown, so
        // skipping drawScanline cuts a large constant off the dominant cost. Set
        // after reset() — which zeroes frameskip — and it sticks (frameskip isn't
        // serialized).
        core.as_mut().gba_mut().set_frameskip(i32::MAX);

        core.as_mut().load_state(initial_state)?;

        Ok(Stepper {
            core,
            state,
            hooks,
            match_type,
            local_player_index,
        })
    }

    /// Rewind the core to `state` before a rollback re-sim. The caller positions
    /// the shadow alongside.
    pub fn restore(&mut self, state: &mgba::state::State) -> anyhow::Result<()> {
        self.core.as_mut().load_state(state)?;
        Ok(())
    }

    /// Advance exactly one tick from where the core is parked (`current_tick`),
    /// applying `input` and capturing the boundary snapshot at `current_tick +
    /// 1`. `last_local_packet` is this side's outgoing link packet at
    /// `current_tick` (seeds the link exchange); `apply_shadow_input` resolves
    /// the remote packet by co-simulating the shadow. Drives the core until the
    /// per-game stepper trap captures the boundary snapshot.
    pub fn step(
        &mut self,
        input: (PartialInput, PartialInput),
        current_tick: u32,
        last_local_packet: &[u8],
        apply_shadow_input: Box<dyn FnMut(u32, (Input, PartialInput)) -> anyhow::Result<Vec<u8>> + Send>,
    ) -> anyhow::Result<StepperResult> {
        self.hooks.prepare_for_fastforward(self.core.as_mut());
        *self.state.0.lock().unwrap() = Some(InnerState::for_fastforward(
            self.match_type,
            self.local_player_index,
            vec![input],
            current_tick,
            last_local_packet.to_vec(),
            apply_shadow_input,
        ));

        loop {
            {
                let mut guard = self.state.0.lock().unwrap();
                let inner = guard.as_mut().unwrap();
                if inner.has_captured_snapshot() {
                    return Ok(guard.take().expect("state").into_stepper_result());
                }
                let _ = inner.take_error();
            }
            self.core.as_mut().run_loop();
            let mut guard = self.state.0.lock().unwrap();
            if let Some(err) = guard.as_mut().expect("state").take_error() {
                guard.take();
                return Err(anyhow::format_err!("replayer: {}", err));
            }
        }
    }
}
