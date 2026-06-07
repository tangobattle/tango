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

/// Shared per-Match emulator machinery for the per-frame stepper traps: a
/// dedicated headless core plus the drive loop that runs the trap set over a
/// known input window and captures one boundary snapshot at `capture_tick`. The
/// two public steppers wrap this and differ only in how they position the core
/// before a run — [`RunStepper`] reloads a seed state each call; [`ResumeStepper`]
/// never reloads and runs forward from where the previous call parked.
struct StepperCore {
    core: mgba::core::Core,
    state: State,
    hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
    match_type: (u8, u8),
    local_player_index: u8,
}

impl StepperCore {
    fn new(
        rom: &[u8],
        hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
        match_type: (u8, u8),
        local_player_index: u8,
        initial_state: Option<&mgba::state::State>,
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
        // Headless re-sim core: never rasterize. Its pixels are never shown,
        // and the round re-runs the FF every frame, so skipping drawScanline
        // cuts a large constant off the dominant cost. Set after reset() —
        // which zeroes frameskip — and it sticks (frameskip isn't serialized).
        core.as_mut().gba_mut().set_frameskip(i32::MAX);

        if let Some(initial_state) = initial_state {
            core.as_mut().load_state(initial_state)?;
        }

        Ok(StepperCore {
            core,
            state,
            hooks,
            match_type,
            local_player_index,
        })
    }

    /// Install the input window and re-arm the per-game traps for a run.
    /// `prepare_for_fastforward` rewinds the PC to `main_read_joyflags` so the
    /// next `run_loop` re-fires the read at the parked position; the shadow
    /// calls it on its warm core every `apply_input` (see `shadow/mod.rs`), so it
    /// is safe to call without a preceding `load_state`.
    fn arm(
        &mut self,
        inputs: Vec<(PartialInput, PartialInput)>,
        current_tick: u32,
        last_local_packet: &[u8],
        apply_shadow_input: Box<dyn FnMut(u32, (Input, PartialInput)) -> anyhow::Result<Vec<u8>> + Send>,
    ) {
        self.hooks.prepare_for_fastforward(self.core.as_mut());
        *self.state.0.lock().unwrap() = Some(InnerState::for_fastforward(
            self.match_type,
            self.local_player_index,
            inputs,
            current_tick,
            last_local_packet.to_vec(),
            apply_shadow_input,
        ));
    }

    /// Drive the core until the per-game stepper trap captures the boundary
    /// snapshot, then return the run's result.
    fn run_to_capture(&mut self) -> anyhow::Result<StepperResult> {
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

/// Cold-start stepper: loads a seed save state and runs the input window from it
/// on every call. Used for the throwaway speculative fork, which is reloaded
/// from the settled checkpoint each frame and may diverge freely. Only supports
/// [`run_until`](Self::run_until); it carries no seed state of its own, since
/// every run supplies one.
pub struct RunStepper(StepperCore);

impl RunStepper {
    pub fn new(
        rom: &[u8],
        hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
        match_type: (u8, u8),
        local_player_index: u8,
    ) -> anyhow::Result<Self> {
        Ok(RunStepper(StepperCore::new(rom, hooks, match_type, local_player_index, None)?))
    }

    /// Cold start: load the seed state, then run the input window to capture.
    pub fn run_until(
        &mut self,
        state: &mgba::state::State,
        inputs: Vec<(PartialInput, PartialInput)>,
        current_tick: u32,
        last_local_packet: &[u8],
        apply_shadow_input: Box<dyn FnMut(u32, (Input, PartialInput)) -> anyhow::Result<Vec<u8>> + Send>,
    ) -> anyhow::Result<StepperResult> {
        self.0.core.as_mut().load_state(state)?;
        self.0.arm(inputs, current_tick, last_local_packet, apply_shadow_input);
        self.0.run_to_capture()
    }
}

/// Forward-only stepper: seeded once at construction and never reloaded, it runs
/// each input window on from wherever the previous call parked the core. Used
/// for the authoritative settle re-sim, which advances monotonically in lockstep
/// with the shadow. Only supports [`resume_until`](Self::resume_until).
pub struct ResumeStepper(StepperCore);

impl ResumeStepper {
    pub fn new(
        rom: &[u8],
        hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
        match_type: (u8, u8),
        local_player_index: u8,
        initial_state: &mgba::state::State,
    ) -> anyhow::Result<Self> {
        Ok(ResumeStepper(StepperCore::new(
            rom,
            hooks,
            match_type,
            local_player_index,
            Some(initial_state),
        )?))
    }

    /// Forward-only continuation for the authoritative settle core. The core is
    /// already parked at `current_tick` from the previous run's capture —
    /// `end_run_loop` halts exactly at the boundary `main_read_joyflags` with r4
    /// unset (see e.g. `game/bn6/stepper.rs`) — so there is NO `load_state`; we
    /// just re-arm the next input window and run on. Sound only because settles
    /// advance monotonically and never rewind: the caller must guarantee the
    /// core is parked at `current_tick`.
    pub fn resume_until(
        &mut self,
        inputs: Vec<(PartialInput, PartialInput)>,
        current_tick: u32,
        last_local_packet: &[u8],
        apply_shadow_input: Box<dyn FnMut(u32, (Input, PartialInput)) -> anyhow::Result<Vec<u8>> + Send>,
    ) -> anyhow::Result<StepperResult> {
        self.0.arm(inputs, current_tick, last_local_packet, apply_shadow_input);
        self.0.run_to_capture()
    }
}
