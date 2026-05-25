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

/// Save snapshot at a specific tick, with the local emulator's outgoing
/// link-cable packet for that tick. Both Fastforwarder and replay use this.
#[derive(Clone)]
pub struct CommittedState {
    pub state: Box<mgba::state::State>,
    pub tick: u32,
    pub packet: Vec<u8>,
}
