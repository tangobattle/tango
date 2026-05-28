/// Outcome from primary's perspective: did the local player win or lose this
/// round? (Draws are mapped to win/loss by [`Round::on_draw_outcome`] before
/// reaching this enum.)
#[derive(PartialEq, Debug, Clone, Copy)]
pub enum BattleOutcome {
    Loss,
    Win,
}

/// Match-wide identity. Both peers compute these to identical values from the
/// shared protocol state, then carry them through Match → Shadow → Round.
#[derive(Clone, Copy)]
pub struct MatchIdentity {
    pub match_type: (u8, u8),
    pub is_offerer: bool,
    pub local_player_index: u8,
}

/// Replay sink: a writer, or none if not recording.
pub struct ReplayConfig {
    pub writer: Option<crate::replay::Writer>,
}

/// Save snapshot from the FF, paired with the local emulator's outgoing
/// link-cable packet at the moment of capture. Both Fastforwarder and replay
/// use this.
#[derive(Clone)]
pub struct CommittedState {
    pub state: Box<mgba::state::State>,
    /// `game.current_tick` at the moment the snapshot was captured — i.e. the
    /// tick the game is *about to process next*, an exclusive upper bound of
    /// what's already been simulated. For `Round::settled_state` this is the
    /// display target, capped at `commit_frontier − 1`.
    pub tick: u32,
    pub packet: Vec<u8>,
}
