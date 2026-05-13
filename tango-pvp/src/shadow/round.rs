use crate::input::{Input, Pair, PartialInput};

/// A shadow input pair queued by `Shadow::apply_input` for the next per-game
/// trap to consume, plus the tick the primary expected the shadow to process
/// it at. The expected tick lets per-game traps detect the "shadow advanced
/// one tick before the trap fired" race.
pub struct PendingShadowInput {
    pub expected_tick: u32,
    pub pair: Pair<Input, PartialInput>,
}

/// `pending_remote_packet`'s payload bundled with the tick at which a
/// consumer should expect to see it. Setters record `current_tick + 1`;
/// consumers verify `target_tick == current_tick`.
struct RemotePacket {
    target_tick: u32,
    packet: Vec<u8>,
}

/// State for a single shadow-emulator round. Per-game shadow traps
/// drive this between [`State::start_round`](super::State::start_round) and
/// [`State::end_round`](super::State::end_round).
pub struct Round {
    pub(super) current_tick: u32,
    pub(super) local_player_index: u8,
    pub(super) first_committed_state: Option<Box<mgba::state::State>>,
    pending_shadow_input: Option<PendingShadowInput>,
    pending_remote_packet: Option<RemotePacket>,
    input_injected: bool,
}

impl Round {
    pub(super) fn new(local_player_index: u8) -> Self {
        Self {
            current_tick: 0,
            local_player_index,
            first_committed_state: None,
            pending_shadow_input: None,
            pending_remote_packet: None,
            input_injected: false,
        }
    }

    pub fn current_tick(&self) -> u32 {
        self.current_tick
    }

    pub fn increment_current_tick(&mut self) {
        self.current_tick += 1;
    }

    pub fn local_player_index(&self) -> u8 {
        self.local_player_index
    }

    pub fn remote_player_index(&self) -> u8 {
        1 - self.local_player_index
    }

    pub fn set_first_committed_state(&mut self, state: Box<mgba::state::State>, packet: &[u8]) {
        self.first_committed_state = Some(state);
        self.pending_remote_packet = Some(RemotePacket {
            target_tick: 0,
            packet: packet.to_vec(),
        });
    }

    pub fn has_first_committed_state(&self) -> bool {
        self.first_committed_state.is_some()
    }

    pub(super) fn set_pending_shadow_input(&mut self, expected_tick: u32, pair: Pair<Input, PartialInput>) {
        self.pending_shadow_input = Some(PendingShadowInput { expected_tick, pair });
    }

    pub fn take_shadow_input(&mut self) -> Option<PendingShadowInput> {
        self.pending_shadow_input.take()
    }

    pub fn peek_shadow_input(&self) -> Option<&PendingShadowInput> {
        self.pending_shadow_input.as_ref()
    }

    pub fn set_remote_packet(&mut self, packet: Vec<u8>) {
        self.pending_remote_packet = Some(RemotePacket {
            target_tick: self.current_tick + 1,
            packet,
        });
    }

    pub fn peek_remote_packet(&self) -> Option<Vec<u8>> {
        self.pending_remote_packet.as_ref().map(|p| p.packet.clone())
    }

    /// Verify the buffered remote packet was queued for the current tick.
    /// Per-game shadow traps call this before consuming the packet.
    pub fn check_remote_packet_at_current_tick(&self) -> anyhow::Result<()> {
        if let Some(p) = self.pending_remote_packet.as_ref() {
            if p.target_tick != self.current_tick {
                anyhow::bail!(
                    "remote packet tick mismatch: stored for tick {}, current tick {}",
                    p.target_tick,
                    self.current_tick,
                );
            }
        }
        Ok(())
    }

    pub fn set_input_injected(&mut self) {
        self.input_injected = true;
    }

    pub fn take_input_injected(&mut self) -> bool {
        std::mem::replace(&mut self.input_injected, false)
    }
}

/// Wraps the optional shadow round and the result-arrived flag.
pub struct RoundState {
    pub round: Option<Round>,
    pub result_is_in: bool,
}

impl RoundState {
    pub fn set_result_is_in(&mut self) {
        self.result_is_in = true;
    }
}
