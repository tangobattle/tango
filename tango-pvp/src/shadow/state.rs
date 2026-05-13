use std::sync::Arc;

use parking_lot::{Mutex, MutexGuard};

use super::round::{Round, RoundState};

/// Snapshot of the shadow emulator at a specific tick, captured by per-game
/// shadow traps and consumed by [`super::Shadow::apply_input`] /
/// [`super::Shadow::advance_until_round_end`] to advance the visible primary.
pub(super) struct AppliedState {
    pub(super) tick: u32,
    pub(super) state: Box<mgba::state::State>,
}

pub(super) struct InnerState {
    match_type: (u8, u8),
    is_offerer: bool,
    local_player_index: u8,
    pub(super) round_state: Mutex<RoundState>,
    pub(super) rng: Mutex<rand_pcg::Mcg128Xsl64>,
    pub(super) applied_state: Mutex<Option<AppliedState>>,
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
    ) -> State {
        State(Arc::new(InnerState {
            match_type,
            is_offerer,
            local_player_index,
            rng: Mutex::new(rng),
            round_state: Mutex::new(RoundState {
                round: None,
                result_is_in: false,
            }),
            applied_state: Mutex::new(None),
            error: Mutex::new(None),
        }))
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
        self.0.rng.lock()
    }

    pub fn lock_round_state(&self) -> MutexGuard<'_, RoundState> {
        self.0.round_state.lock()
    }

    /// Allocate a fresh [`Round`] for the new shadow round. Shadow shares the
    /// same `local_player_index` as primary; per-game shadow traps flip via
    /// `remote_player_index()` at the call site to return values from the
    /// peer's perspective.
    pub fn start_round(&self) {
        let mut round_state = self.0.round_state.lock();
        let local_player_index = self.0.local_player_index;
        log::info!(
            "starting shadow round: local_player_index = {}",
            local_player_index
        );
        round_state.round = Some(Round::new(local_player_index));
        round_state.result_is_in = false;
    }

    pub fn end_round(&self) {
        log::info!("shadow round ended");
        let mut round_state = self.0.round_state.lock();
        round_state.round = None;
        round_state.result_is_in = false;
    }

    pub fn set_anyhow_error(&self, err: anyhow::Error) {
        *self.0.error.lock() = Some(err);
    }

    pub fn set_applied_state(&self, state: Box<mgba::state::State>, tick: u32) {
        *self.0.applied_state.lock() = Some(AppliedState { tick, state });
    }
}
