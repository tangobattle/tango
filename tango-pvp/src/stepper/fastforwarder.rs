use crate::input::{Input, PartialInput};

use super::state::{InnerState, State};
use super::types::RoundResult;

/// Output of a single Fastforwarder run.
pub struct FastforwardResult {
    /// State captured at `capture_tick` post-peek: the per-game stepper trap
    /// fires `main_read_joyflags`, sets r4 to the peeked input's local
    /// joyflags, and snapshots from there — so loading this state into either
    /// the live core (immediate render) or another `fastforward` run (as the
    /// next base state) resumes with the right local-joyflags register.
    pub snapshot: crate::battle::Snapshot,
    pub round_result: Option<RoundResult>,
    pub output_pairs: Vec<(Input, Input)>,
}

/// Per-Match emulator dedicated to running the per-frame stepper traps over a
/// known input window. Each [`fastforward`](Fastforwarder::fastforward) call
/// loads a saved state, processes the input pairs, and returns a single fresh
/// save snapshot at `capture_tick`.
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

        Ok(Fastforwarder {
            core,
            state,
            hooks,
            match_type,
            local_player_index,
        })
    }

    pub fn fastforward(
        &mut self,
        state: &mgba::state::State,
        inputs: Vec<(PartialInput, PartialInput)>,
        peeked: (PartialInput, PartialInput),
        current_tick: u32,
        last_local_packet: &[u8],
        apply_shadow_input: Box<dyn FnMut(u32, (Input, PartialInput)) -> anyhow::Result<Vec<u8>> + Send>,
    ) -> anyhow::Result<FastforwardResult> {
        self.core.as_mut().load_state(state)?;
        self.hooks.prepare_for_fastforward(self.core.as_mut());

        *self.state.0.lock().unwrap() = Some(InnerState::for_fastforward(
            self.match_type,
            self.local_player_index,
            inputs,
            peeked,
            current_tick,
            last_local_packet.to_vec(),
            apply_shadow_input,
        ));

        loop {
            {
                let mut guard = self.state.0.lock().unwrap();
                let inner = guard.as_mut().unwrap();
                if inner.has_captured_snapshot() {
                    return Ok(guard.take().expect("state").into_fastforward_result());
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
