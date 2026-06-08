//! The mgba/Battle-Network adapter for the [`getgud`] rollback engine.
//!
//! getgud is plain rollback over an opaque state + input; everything
//! link-cable lives here. The opponent's per-tick packets aren't on the wire,
//! so [`MgbaSimulator`] derives them inside each step by co-simulating the
//! opponent (the [`Shadow`](crate::shadow::Shadow)) — for *both* confirmed
//! settles and speculative ticks, driven by the engine's predicted-then-confirmed
//! remote joyflags. Because the packet is always shadow-derived (never faked),
//! a speculation whose predicted joyflags matched the real ones is byte-exact
//! and the engine can promote it with no re-simulation; only a genuine
//! misprediction triggers a [`restore`]+re-step rollback of both cores.
//!
//! - [`MgbaWorld`] pins the engine's type axes: [`MgbaState`] (the primary +
//!   shadow snapshots and our in-flight outgoing packet) and [`PartialInput`]
//!   (joyflags).
//! - [`MgbaSimulator`] wraps the single [`Stepper`](crate::stepper::Stepper)
//!   core and owns the shadow.
//! - [`MgbaPredictor`] guesses the remote *joyflags* (held A/B); the packet then
//!   falls out of the shadow co-sim.
//!
//! The chosen display state is loaded into the live core — and the time-sync
//! skew turned into a frame-rate target via [`Throttler`](super::throttler::Throttler)
//! — by [`Round`](super::Round), not here.
//!
//! [`restore`]: crate::stepper::Stepper::restore

use std::sync::{Arc, Mutex as SyncMutex};

use crate::input::{Input, PartialInput};

/// Binds the engine's generic type axes to this crate's concrete types.
pub struct MgbaWorld;

impl getgud::World for MgbaWorld {
    /// Joyflags — what's queued and what crosses the wire.
    type Input = PartialInput;
    type State = MgbaState;
    type Error = anyhow::Error;
}

/// The engine's opaque checkpoint state: the primary stepper's mgba save state,
/// the shadow's snapshot (so a rollback rewinds the opponent co-sim in lockstep),
/// our own outgoing link-cable packet at that tick (needed to continue the
/// exchange on resume), and the tick the bundle is poised at. The engine treats
/// this as a blob; the simulator reads `tick` to decide whether a `restore` is a
/// real rewind or a no-op resume.
pub struct MgbaState {
    pub primary: Box<mgba::state::State>,
    pub outgoing: Vec<u8>,
    pub shadow_snapshot: crate::shadow::ShadowSnapshot,
    pub tick: u32,
}

/// Per-tick remote-packet resolver handed to the stepper.
type Resolver = Box<dyn FnMut(u32, (Input, PartialInput)) -> anyhow::Result<Vec<u8>> + Send>;

/// [`getgud::Simulator`] over the single per-frame [`Stepper`] core plus the
/// shadow. Every [`step`](getgud::Simulator::step) co-simulates the opponent for
/// that tick (real packet from real-or-predicted remote joyflags) and captures a
/// boundary snapshot of both cores. [`restore`](getgud::Simulator::restore)
/// rewinds both cores to a saved bundle before a rollback re-sim — but is a no-op
/// when the cores are already parked at that tick, so steady-state settles stay
/// forward-only.
///
/// [`Stepper`]: crate::stepper::Stepper
pub struct MgbaSimulator {
    pub stepper: crate::stepper::Stepper,
    pub shadow: Arc<SyncMutex<crate::shadow::Shadow>>,
    /// The tick both cores are currently parked at.
    pub parked_tick: u32,
    /// This side's outgoing link packet at the parked tick — seeds the next
    /// step's link exchange.
    pub last_outgoing: Vec<u8>,
}

impl getgud::Simulator<MgbaWorld> for MgbaSimulator {
    fn restore(&mut self, state: &MgbaState) -> anyhow::Result<()> {
        // Already parked here — either no speculation moved the cores since this
        // tick settled, or every speculation up to it was promoted. The cores and
        // `last_outgoing` already hold `state`, so skip the reloads; this keeps
        // steady-state settles forward-only (no `load_state` per frame).
        if self.parked_tick == state.tick {
            return Ok(());
        }
        self.stepper.restore(&state.primary)?;
        self.shadow.lock().unwrap().load_state(&state.shadow_snapshot)?;
        self.parked_tick = state.tick;
        self.last_outgoing = state.outgoing.clone();
        Ok(())
    }

    fn step(&mut self, input: (PartialInput, PartialInput)) -> anyhow::Result<Option<MgbaState>> {
        // Co-simulate the opponent for this tick: the resolver runs the shadow
        // forward over the (real or predicted) remote joyflags to derive the
        // remote packet. The shadow advances in lockstep with the stepper and is
        // rewound by `restore`, so this is identical whether the tick is a
        // confirmed settle or a speculative one.
        let resolver: Resolver = {
            let shadow = self.shadow.clone();
            Box::new(move |tick, ip| shadow.lock().unwrap().apply_input(tick, ip))
        };
        let last_outgoing = self.last_outgoing.clone();
        let result = self.stepper.step(input, self.parked_tick, &last_outgoing, resolver)?;
        let shadow_snapshot = self.shadow.lock().unwrap().save_state()?;

        // Advance the parked position even when ending — the cores have moved, so
        // a later `restore` to an earlier tick must see a stale park and reload.
        self.parked_tick = result.snapshot.tick;
        self.last_outgoing = result.snapshot.packet.clone();

        // The per-game round-end traps fire one tick after the round-end frame is
        // captured, so the step that reports a round result is the first one past
        // the live tail: there is no further live state to advance into.
        if result.round_result.is_some() {
            return Ok(None);
        }

        Ok(Some(MgbaState {
            primary: result.snapshot.state,
            outgoing: result.snapshot.packet,
            shadow_snapshot,
            tick: result.snapshot.tick,
        }))
    }
}

/// [`getgud::Predictor`]: the remote joyflags we assume hold during speculation
/// — just the held keys (A/B), nothing transient. The packet half then falls out
/// of the shadow co-sim in [`MgbaSimulator::step`].
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
