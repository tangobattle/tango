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

use getgud::SimResult;

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
    /// Forward-only authoritative re-sim: runs settles in lockstep with the
    /// shadow and is never reloaded after seeding, mirroring the shadow's own
    /// forward-only discipline. Parked at the last settled tick between calls.
    pub authoritative_ff: crate::stepper::Fastforwarder,
    /// Throwaway speculative fork: reloaded from the settled checkpoint every
    /// frame and run forward with predicted remote packets. Never touches the
    /// shadow, so it can diverge freely without disturbing the trunk.
    pub speculative_ff: crate::stepper::Fastforwarder,
    pub shadow: Arc<SyncMutex<crate::shadow::Shadow>>,
    pub hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
    /// The last remote packet a settle resolved — the seed `predict_rx` advances
    /// from during a speculative tail.
    pub last_remote_packet: Vec<u8>,
    /// Tick the `authoritative_ff` core is parked at, or `None` before the first
    /// settle. A settle warm-continues when this equals the engine's seed tick;
    /// otherwise it cold-seeds via `load_state`. The equality guard self-heals:
    /// any discontinuity (round boundary, unexpected seed) just falls to cold.
    pub authoritative_parked_tick: Option<u32>,
}

impl getgud::Simulator<MgbaWorld> for MgbaSimulator {
    fn simulate(
        &mut self,
        base: &MgbaState,
        base_tick: u32,
        inputs: Vec<(PartialInput, PartialInput)>,
        speculative: bool,
    ) -> anyhow::Result<SimResult<MgbaWorld>> {
        let input_count = inputs.len();

        // The fastforwarder advances through `inputs`, then captures poised at
        // the start of the boundary tick (one past the last applied) without
        // stepping it — mirroring getgud's contract. The boundary tick's local
        // joyflags are not baked into the snapshot; the live core primes them
        // from `Frame::input` via `inject_joyflags_on_primary_snapshot`.
        let result = if speculative {
            // The fork: reload the settled state and run the tail with predicted
            // packets. `predict_rx` advances from the last settled remote packet
            // (returns-then-advances); never locks the shadow, so the throwaway
            // tail can diverge without disturbing the authoritative core.
            let hooks = self.hooks;
            let mut packet = self.last_remote_packet.clone();
            hooks.predict_rx(&mut packet);
            let resolver: Resolver = Box::new(move |_tick, _ip| {
                let out = packet.clone();
                hooks.predict_rx(&mut packet);
                Ok(out)
            });
            self.speculative_ff
                .fastforward(&base.core, inputs, base_tick, &base.outgoing, resolver)?
        } else {
            // The trunk: co-simulate the opponent (shadow) over the local side's
            // just-produced packet, advancing forward-only in lockstep with it.
            let resolver: Resolver = {
                let shadow = self.shadow.clone();
                Box::new(move |tick, ip| shadow.lock().unwrap().apply_input(tick, ip))
            };
            let result = if self.authoritative_parked_tick == Some(base_tick) {
                // Warm: the core is already parked at `base_tick` from the
                // previous settle's capture — continue without reloading.
                self.authoritative_ff
                    .fastforward_resume(inputs, base_tick, &base.outgoing, resolver)?
            } else {
                // Cold: first settle of the round, or a discontinuity. Seed the
                // core from the engine's settled checkpoint.
                self.authoritative_ff
                    .fastforward(&base.core, inputs, base_tick, &base.outgoing, resolver)?
            };
            // Capture parked the core at the new settled tick; warm next time.
            self.authoritative_parked_tick = Some(result.snapshot.tick);
            // A settle defines the new last-confirmed remote packet for the next
            // speculative tail's prediction.
            if let Some((_local, remote)) = result.output_pairs.last() {
                self.last_remote_packet = remote.packet.clone();
            }
            result
        };

        // `round_result.tick` is the absolute tick the round ended at, in the
        // same per-pair units as `base_tick`; turn it into a count of the pairs
        // consumed before the end. Live (no result) means all of them.
        let committed = match result.round_result {
            Some(rr) => (rr.tick.saturating_sub(base_tick) as usize).min(input_count),
            None => input_count,
        };

        Ok(SimResult {
            state: MgbaState {
                core: result.snapshot.state,
                outgoing: result.snapshot.packet,
            },
            committed,
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

/// [`getgud::Logger`] that logs committed joyflags into the replay file.
/// Packets aren't stored — the playback stepper re-derives them.
pub struct ReplayLogger {
    pub writer: Arc<SyncMutex<Option<crate::replay::Writer>>>,
    pub local_player_index: u8,
}

impl getgud::Logger<MgbaWorld> for ReplayLogger {
    fn log(&mut self, pair: &(PartialInput, PartialInput)) {
        if let Some(writer) = self.writer.lock().unwrap().as_mut() {
            writer.write_input(self.local_player_index, pair).expect("write input");
        }
    }
}
