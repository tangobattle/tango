//! The mgba/Battle-Network adapter for the [`getgud`] rollback engine.
//!
//! getgud is plain rollback over an opaque state + input; everything
//! link-cable lives here. The opponent's per-tick packets aren't on the wire,
//! so [`MgbaSimulator`] derives them inside its run — by co-simulating the
//! opponent (the [`Shadow`](crate::shadow::Shadow)) for a real settle, or by
//! the per-game `predict_rx` for a throwaway speculative tail. The engine never
//! sees a packet; it only flips `speculative`.
//!
//! - [`MgbaWorld`] pins the engine's type axes: [`MgbaState`] (mgba save state
//!   + our in-flight outgoing packet) and [`PartialInput`] (joyflags).
//! - [`MgbaSimulator`] wraps the [`Fastforwarder`](crate::stepper::Fastforwarder)
//!   and owns the shadow.
//! - [`MgbaPredictor`] guesses the remote *joyflags* (held A/B); the packet is
//!   the simulator's own business.
//!
//! The chosen display state is loaded into the live core — and the time-sync
//! skew turned into a frame-rate target via [`Throttler`](super::throttler::Throttler)
//! — by [`Round`](super::Round), not here.

use std::sync::{Arc, Mutex as SyncMutex};

use getgud::{SimResult, Snapshot};

use crate::input::{Input, PartialInput};

/// Binds the engine's generic type axes to this crate's concrete types.
pub struct MgbaWorld;

impl getgud::World for MgbaWorld {
    /// Joyflags — what's queued and what crosses the wire.
    type Input = PartialInput;
    type State = MgbaState;
    type Error = anyhow::Error;
}

/// The engine's opaque checkpoint state: the mgba save state plus our own
/// outgoing link-cable packet at that tick (needed to continue the exchange on
/// resume). The engine treats this as a blob.
pub struct MgbaState {
    pub core: Box<mgba::state::State>,
    pub outgoing: Vec<u8>,
}

/// Per-tick remote-packet resolver handed to the fastforwarder.
type Resolver = Box<dyn FnMut(u32, (Input, PartialInput)) -> anyhow::Result<Vec<u8>> + Send>;

/// [`getgud::Simulator`] over the per-frame [`Fastforwarder`]. Owns the shadow
/// and resolves each tick's remote packet itself: a settle (`speculative =
/// false`) co-simulates the opponent and advances it; a speculative tail
/// predicts packets via `predict_rx` and never touches the shadow.
pub struct MgbaSimulator {
    pub ff: crate::stepper::Fastforwarder,
    pub shadow: Arc<SyncMutex<crate::shadow::Shadow>>,
    pub hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
    /// The last remote packet a settle resolved — the seed `predict_rx` advances
    /// from during a speculative tail.
    pub last_remote_packet: Vec<u8>,
}

impl getgud::Simulator<MgbaWorld> for MgbaSimulator {
    fn simulate(
        &mut self,
        base: &Snapshot<MgbaWorld>,
        committed: Vec<(PartialInput, PartialInput)>,
        peeked: (PartialInput, PartialInput),
        speculative: bool,
    ) -> anyhow::Result<SimResult<MgbaWorld>> {
        let resolver: Resolver = if speculative {
            // Predicted packets: advance `predict_rx` from the last settled
            // remote packet (returns-then-advances; never touches the shadow).
            let hooks = self.hooks;
            let mut packet = self.last_remote_packet.clone();
            hooks.predict_rx(&mut packet);
            Box::new(move |_tick, _ip| {
                let out = packet.clone();
                hooks.predict_rx(&mut packet);
                Ok(out)
            })
        } else {
            // Real packets: co-simulate the opponent over the local side's
            // just-produced packet.
            let shadow = self.shadow.clone();
            Box::new(move |tick, ip| shadow.lock().unwrap().apply_input(tick, ip))
        };

        // The peek threads straight through: the fastforwarder advances through
        // `committed`, then captures at the peeked tick (priming r4 with its
        // local joyflags) without integrating it — mirroring getgud's contract.
        let result = self
            .ff
            .fastforward(&base.state.core, committed, peeked, base.tick, &base.state.outgoing, resolver)?;

        // A settle defines the new last-confirmed remote packet for the next
        // speculative tail's prediction.
        if !speculative {
            if let Some((_local, remote)) = result.output_pairs.last() {
                self.last_remote_packet = remote.packet.clone();
            }
        }

        Ok(SimResult {
            snapshot: Snapshot {
                state: MgbaState {
                    core: result.snapshot.state,
                    outgoing: result.snapshot.packet,
                },
                tick: result.snapshot.tick,
            },
            commit_before: result.round_result.map(|rr| rr.tick),
        })
    }
}

/// [`getgud::Predictor`]: the remote joyflags we assume hold during speculation
/// — just the held keys (A/B), nothing transient. The packet half of the
/// prediction lives in [`MgbaSimulator`].
pub struct MgbaPredictor;

impl getgud::Predictor<MgbaWorld> for MgbaPredictor {
    fn predict(&self, last_remote: &PartialInput) -> PartialInput {
        const HELD_KEYS: u16 = mgba::input::keys::A as u16 | mgba::input::keys::B as u16;
        PartialInput {
            joyflags: last_remote.joyflags & HELD_KEYS,
        }
    }
}

/// [`getgud::CommitObserver`] that records committed joyflags into the replay
/// file. Packets aren't stored — the playback stepper re-derives them.
pub struct ReplayObserver {
    pub writer: Arc<SyncMutex<Option<crate::replay::Writer>>>,
    pub local_player_index: u8,
}

impl getgud::CommitObserver<MgbaWorld> for ReplayObserver {
    fn on_commit(&mut self, _tick: u32, pair: &(PartialInput, PartialInput)) {
        if let Some(writer) = self.writer.lock().unwrap().as_mut() {
            writer.write_input(self.local_player_index, pair).expect("write input");
        }
    }
}
