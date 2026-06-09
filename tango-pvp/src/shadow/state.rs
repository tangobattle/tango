use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use super::round::{Round, RoundState};

pub(super) struct InnerState {
    match_type: (u8, u8),
    is_offerer: bool,
    local_player_index: u8,
    /// Chip-select deliberation cap, in battle ticks; `None` disables the timer
    /// (e.g. replay reconstruction, so the golden suite stays byte-identical).
    custom_screen_tick_limit: Option<u32>,
    pub(super) round_state: Mutex<RoundState>,
    pub(super) rng: Mutex<rand_pcg::Mcg128Xsl64>,
    pub(super) input_applied: AtomicBool,
    pub(super) error: Mutex<Option<anyhow::Error>>,
}

/// Shared handle to the shadow emulator's `InnerState`. Per-game shadow traps
/// clone this and lock the relevant submutexes inside their closure.
#[derive(Clone)]
pub struct State(pub(super) Arc<InnerState>);

impl State {
    pub fn new(
        match_type: (u8, u8),
        is_offerer: bool,
        local_player_index: u8,
        rng: rand_pcg::Mcg128Xsl64,
        custom_screen_tick_limit: Option<u32>,
    ) -> State {
        State(Arc::new(InnerState {
            match_type,
            is_offerer,
            local_player_index,
            custom_screen_tick_limit,
            rng: Mutex::new(rng),
            round_state: Mutex::new(RoundState {
                round: None,
                result_is_in: false,
            }),
            input_applied: AtomicBool::new(false),
            error: Mutex::new(None),
        }))
    }

    pub fn custom_screen_tick_limit(&self) -> Option<u32> {
        self.0.custom_screen_tick_limit
    }

    pub fn match_type(&self) -> (u8, u8) {
        self.0.match_type
    }

    pub fn is_offerer(&self) -> bool {
        self.0.is_offerer
    }

    pub fn local_player_index(&self) -> u8 {
        self.0.local_player_index
    }

    pub fn lock_rng(&self) -> MutexGuard<'_, rand_pcg::Mcg128Xsl64> {
        self.0.rng.lock().unwrap()
    }

    pub fn lock_round_state(&self) -> MutexGuard<'_, RoundState> {
        self.0.round_state.lock().unwrap()
    }

    /// Allocate a fresh [`Round`] for the new shadow round. Shadow shares the
    /// same `local_player_index` as primary; per-game shadow traps flip via
    /// `remote_player_index()` at the call site to return values from the
    /// peer's perspective.
    pub fn start_round(&self) {
        let mut round_state = self.0.round_state.lock().unwrap();
        let local_player_index = self.0.local_player_index;
        log::info!("starting shadow round: local_player_index = {}", local_player_index);
        round_state.round = Some(Round::new(local_player_index));
        round_state.result_is_in = false;
    }

    pub fn end_round(&self) {
        log::info!("shadow round ended");
        let mut round_state = self.0.round_state.lock().unwrap();
        round_state.round = None;
        round_state.result_is_in = false;
    }

    pub fn set_anyhow_error(&self, err: anyhow::Error) {
        *self.0.error.lock().unwrap() = Some(err);
    }

    /// Signal that the pending shadow input has been consumed and the core has
    /// run forward to the next tick's `main_read_joyflags` (where the per-game
    /// trap calls `end_run_loop`). [`super::Shadow::apply_input`] polls this to
    /// know its run is done — no snapshot needed, since `end_run_loop` already
    /// parks the core at that tick boundary.
    pub fn set_input_applied(&self) {
        self.0.input_applied.store(true, Ordering::Relaxed);
    }

    pub fn take_input_applied(&self) -> bool {
        self.0.input_applied.swap(false, Ordering::Relaxed)
    }
}
