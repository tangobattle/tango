use std::sync::{Arc, Mutex, MutexGuard};

use super::round::Round;

/// The shadow's mutable engine state, all behind [`State`]'s single lock.
/// The shadow only ever runs on one thread at a time — cross-thread access
/// goes through the `Mutex<Shadow>` around the whole emulator — so this lock
/// never contends; it exists because the per-game trap closures are
/// `'static` and need shared ownership of the state they poke. Traps that
/// touch several fields take the lock once and split-borrow
/// (`let state = &mut *state;`).
pub struct Shared {
    /// The in-progress shadow round, if any. Allocated by
    /// [`State::start_round`], dropped by [`State::end_round`].
    pub round: Option<Round>,
    /// Set when the game's round-end traps report a result. (BN3's jump
    /// table trap fudges link input once this is in.) Engine-internal: the
    /// per-game traps go through [`Shared::signal_result_in`] /
    /// [`Shared::result_reported`] rather than touching the flag directly.
    pub(super) result_is_in: bool,
    pub rng: rand_pcg::Mcg128Xsl64,
    /// Signal that the pending shadow input has been consumed and the core
    /// has run forward to the next tick's `main_read_joyflags`, where the
    /// per-game trap raises this (via [`Shared::signal_input_applied`]) and
    /// calls `end_run_loop` (which parks the core at that tick boundary).
    /// [`Shadow::apply_input`](super::Shadow::apply_input) polls it via
    /// [`State::take_input_applied`] to know its run is done.
    pub(super) input_applied: bool,
}

impl Shared {
    /// Flag that the pending shadow input has been consumed and the core has
    /// reached the next tick's `main_read_joyflags`. Raised by the per-game
    /// shadow trap (which then calls `end_run_loop`).
    pub fn signal_input_applied(&mut self) {
        self.input_applied = true;
    }

    /// Flag that the game's round-end traps have reported a round result.
    pub fn signal_result_in(&mut self) {
        self.result_is_in = true;
    }

    /// Whether the round-end result has been reported (see
    /// [`signal_result_in`](Self::signal_result_in)).
    pub fn result_reported(&self) -> bool {
        self.result_is_in
    }
}

pub(super) struct InnerState {
    match_type: (u8, u8),
    is_offerer: bool,
    local_player_index: u8,
    pub(super) shared: Mutex<Shared>,
    /// Trap error channel, drained by the drive loops after each run burst.
    /// Deliberately outside [`Shared`]: traps report errors mid-body while
    /// holding `&mut Round` borrows into the shared state, so the channel
    /// must not route through that lock.
    pub(super) error: Mutex<Option<anyhow::Error>>,
}

/// Shared handle to the shadow emulator's `InnerState`. Per-game shadow
/// traps clone this and lock the shared state inside their closures.
#[derive(Clone)]
pub struct State(pub(super) Arc<InnerState>);

impl State {
    pub fn new(match_type: (u8, u8), is_offerer: bool, local_player_index: u8, rng: rand_pcg::Mcg128Xsl64) -> State {
        State(Arc::new(InnerState {
            match_type,
            is_offerer,
            local_player_index,
            shared: Mutex::new(Shared {
                round: None,
                result_is_in: false,
                rng,
                input_applied: false,
            }),
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

    pub fn lock(&self) -> MutexGuard<'_, Shared> {
        self.0.shared.lock().unwrap()
    }

    /// Allocate a fresh [`Round`] for the new shadow round. Shadow shares the
    /// same `local_player_index` as primary; per-game shadow traps flip via
    /// `remote_player_index()` at the call site to return values from the
    /// peer's perspective.
    pub fn start_round(&self) {
        let local_player_index = self.0.local_player_index;
        log::info!("starting shadow round: local_player_index = {}", local_player_index);
        let mut shared = self.lock();
        shared.round = Some(Round::new(local_player_index));
        shared.result_is_in = false;
    }

    pub fn end_round(&self) {
        log::info!("shadow round ended");
        let mut shared = self.lock();
        shared.round = None;
        shared.result_is_in = false;
    }

    pub fn set_anyhow_error(&self, err: anyhow::Error) {
        *self.0.error.lock().unwrap() = Some(err);
    }

    /// Discard any queued trap error — used when restoring a snapshot,
    /// where a pending error belongs to the run the restore just threw away.
    pub fn clear_error(&self) {
        *self.0.error.lock().unwrap() = None;
    }

    /// Take-and-clear the [`Shared::input_applied`] signal.
    /// [`super::Shadow::apply_input`] polls this to know its run is done —
    /// no snapshot needed, since `end_run_loop` already parked the core at
    /// the tick boundary.
    pub fn take_input_applied(&self) -> bool {
        std::mem::take(&mut self.lock().input_applied)
    }
}
