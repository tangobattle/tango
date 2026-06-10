use crate::input::{Input, PartialInput};

/// A shadow input pair queued by `Shadow::apply_input` for the next per-game
/// trap to consume.
#[derive(Clone)]
pub struct PendingShadowInput {
    pub pair: (Input, PartialInput),
}

/// `pending_remote_packet`'s payload bundled with the tick at which a
/// consumer should expect to see it. Setters record `current_tick + 1`;
/// consumers verify `target_tick == current_tick`.
#[derive(Clone)]
pub(super) struct RemotePacket {
    pub(super) target_tick: u32,
    pub(super) packet: Vec<u8>,
}

/// The lifecycle of one link exchange through the shadow core: from
/// [`Shadow::apply_input`](super::Shadow::apply_input) queueing a tick's
/// input pair, through the per-game traps consuming it and injecting the rx
/// packets, to the exchange being applied. One explicit machine instead of
/// the old `pending_shadow_input: Option<_>` + `input_injected: bool` pair;
/// the per-game traps drive it through the same method names that used to
/// poke those fields.
///
/// ```text
///   *       ──set_pending_shadow_input──► Queued    (apply_input only)
///   Queued  ──take_shadow_input─────────► Idle      (trap consumed the input)
///   *       ──set_input_injected────────► Applied   (rx injected, next packet buffered)
///   Applied ──take_input_injected───────► Idle      (returns true; else false, no move)
/// ```
///
/// Per-game realities this must allow:
/// - `Idle → Applied`: bn4/5/6/exe45's `copy_input_data_ret` fires ungated,
///   so pre-first-commit and round-end-advance runs "complete" exchanges
///   nothing queued and no caller is waiting for. The resulting signal is
///   discarded as stale by the next `apply_input`.
/// - The whole `Queued → Idle → Applied` cycle inside a single trap fire:
///   bn1/bn2's combined send hooks (two ROM sites) and bn3's (three sites).
///
/// The "exchange applied AND core parked at the next tick boundary" signal
/// that [`Shadow::apply_input`](super::Shadow::apply_input)'s drive loop
/// polls is deliberately NOT a variant here: it is the `input_applied` flag
/// beside the round in the shadow's shared state — per-game traps raise it
/// in the same locked scope that drives the exchange, and the drive loop
/// polls it between run bursts.
#[derive(Clone)]
enum Exchange {
    Idle,
    Queued(PendingShadowInput),
    Applied,
}

/// State for a single shadow-emulator round. Per-game shadow traps
/// drive this between [`State::start_round`](super::State::start_round) and
/// [`State::end_round`](super::State::end_round).
#[derive(Clone)]
pub struct Round {
    pub(super) current_tick: u32,
    pub(super) local_player_index: u8,
    pub(super) first_committed: bool,
    exchange: Exchange,
    pub(super) pending_remote_packet: Option<RemotePacket>,
}

impl Round {
    pub(super) fn new(local_player_index: u8) -> Self {
        Self {
            current_tick: 0,
            local_player_index,
            first_committed: false,
            exchange: Exchange::Idle,
            pending_remote_packet: None,
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

    pub fn set_first_committed(&mut self, packet: &[u8]) {
        self.first_committed = true;
        self.pending_remote_packet = Some(RemotePacket {
            target_tick: 0,
            packet: packet.to_vec(),
        });
    }

    pub fn has_first_committed_state(&self) -> bool {
        self.first_committed
    }

    /// `* → Queued`. Only [`Shadow::apply_input`](super::Shadow::apply_input)
    /// calls this, at the start of an exchange; a leftover `Applied` from an
    /// out-of-band run (round-end advance) is overwritten here, which is what
    /// makes its stale signal harmless.
    pub(super) fn set_pending_shadow_input(&mut self, pair: (Input, PartialInput)) {
        self.exchange = Exchange::Queued(PendingShadowInput { pair });
    }

    /// `Queued → Idle`, returning the input. `None` (no transition) in any
    /// other state — per-game traps use that arm for runs where nothing is
    /// queued (pre-first-commit, round-end advances).
    pub fn take_shadow_input(&mut self) -> Option<PendingShadowInput> {
        match std::mem::replace(&mut self.exchange, Exchange::Idle) {
            Exchange::Queued(pending) => Some(pending),
            other => {
                self.exchange = other;
                None
            }
        }
    }

    pub fn peek_shadow_input(&self) -> Option<&PendingShadowInput> {
        match &self.exchange {
            Exchange::Queued(pending) => Some(pending),
            _ => None,
        }
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

    /// `* → Applied`. Fired by the per-game copy-input-ret / combined send
    /// traps after the rx packets are injected and the next remote packet is
    /// buffered — including from `Idle` on runs with nothing queued (see
    /// [`Exchange`]).
    pub fn set_input_injected(&mut self) {
        self.exchange = Exchange::Applied;
    }

    /// `Applied → Idle`, returning true: the exchange completed and the core
    /// has come back around to `main_read_joyflags`, i.e. it is parked at the
    /// next tick's boundary. Any other state: false, no transition.
    pub fn take_input_injected(&mut self) -> bool {
        if matches!(self.exchange, Exchange::Applied) {
            self.exchange = Exchange::Idle;
            true
        } else {
            false
        }
    }
}

