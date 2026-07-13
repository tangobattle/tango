use std::sync::{Arc, Mutex, MutexGuard};

use super::round::Round;

/// Why a per-game shadow trap parked the core, ending a drive-loop burst.
/// The one channel between trap bodies and the linear drive loops in
/// [`Shadow`](super::Shadow): every `end_run_loop` in the shadow traps goes
/// through [`State::halt`], so each burst the drive loops run comes back
/// with one of these (or the trap error, which rides the same slot) — the
/// loops match on the reason instead of polling flags.
#[derive(Debug)]
pub enum Halt {
    /// The round's first committed state was recorded
    /// (`Round::set_first_committed`); the core is parked exactly there.
    /// Awaited by [`Shadow::advance_until_first_committed_state`].
    ///
    /// [`Shadow::advance_until_first_committed_state`]: super::Shadow::advance_until_first_committed_state
    FirstCommit,
    /// The pending shadow input was consumed, the exchange completed, and
    /// the core has come back around to the next tick's boundary
    /// (`Round::take_input_injected` returned true at `main_read_joyflags`).
    /// Awaited by [`Shadow::finish_apply_input`]; the round-end advance sees
    /// it too (the game keeps exchanging link data through the round-end
    /// screens) and just keeps running.
    ///
    /// [`Shadow::finish_apply_input`]: super::Shadow::finish_apply_input
    InputApplied,
    /// The game reached its round-end entry and the round state was dropped
    /// ([`State::end_round`]); the core is parked at round end. Awaited by
    /// [`Shadow::advance_until_round_end`].
    ///
    /// [`Shadow::advance_until_round_end`]: super::Shadow::advance_until_round_end
    RoundEnded,
}

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
}

impl Shared {
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
    /// Single-slot halt/error channel, taken by the drive loops after each
    /// run burst. Deliberately outside [`Shared`]: traps halt and report
    /// errors mid-body while holding `&mut Round` borrows into the shared
    /// state, so the channel must not route through that lock.
    halt: Mutex<Option<anyhow::Result<Halt>>>,
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
            }),
            halt: Mutex::new(None),
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
        let mut shared = self.lock();
        shared.round = Some(Round::new(local_player_index));
        shared.result_is_in = false;
    }

    pub fn end_round(&self) {
        let mut shared = self.lock();
        shared.round = None;
        shared.result_is_in = false;
    }

    /// Park the core here and say why: record `halt` and end the current
    /// `run_loop` burst. The driving [`Shadow`](super::Shadow) run loop takes
    /// the reason and returns it to its caller, so the linear drive code gets
    /// a typed answer instead of polling flags. `end_run_loop` truncates the
    /// burst at the current instruction, so no further trap can fire before
    /// the drive loop consumes the slot. Safe to call while holding the
    /// [`Shared`] lock (the slot is its own lock).
    pub fn halt(&self, mut core: mgba::core::CoreMutRef, halt: Halt) {
        let mut slot = self.0.halt.lock().unwrap();
        // An error already in the slot wins: the trap that reported it did
        // NOT end the burst, so a later trap in the same burst can land here
        // — its halt describes a run the error has already condemned.
        if !matches!(slot.as_ref(), Some(Err(_))) {
            *slot = Some(Ok(halt));
        }
        drop(slot);
        core.end_run_loop();
    }

    /// Report a trap-invariant failure. Rides the halt slot (overwriting any
    /// pending reason) but does *not* end the burst — the core runs on to the
    /// current batch's natural end, where the drive loop takes the slot and
    /// fails the run.
    pub fn set_anyhow_error(&self, err: anyhow::Error) {
        *self.0.halt.lock().unwrap() = Some(Err(err));
    }

    /// Take-and-clear the halt slot; the drive loops call this after every
    /// burst.
    pub(super) fn take_halt(&self) -> Option<anyhow::Result<Halt>> {
        self.0.halt.lock().unwrap().take()
    }

    /// Discard any queued halt or error — used when restoring a snapshot,
    /// where a pending reason belongs to the run the restore just threw away.
    pub(super) fn clear_halt(&self) {
        *self.0.halt.lock().unwrap() = None;
    }
}
