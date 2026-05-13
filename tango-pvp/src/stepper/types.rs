/// Outcome of a single round, as detected by the per-game `round_end_*` traps.
#[derive(Clone, Copy, PartialEq, serde_repr::Serialize_repr)]
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
