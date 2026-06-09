//! The mgba/Battle-Network adapter for the [`getgud`] rollback engine.
//!
//! getgud is plain rollback over an opaque state + input; everything
//! link-cable lives here. The opponent's per-tick packets aren't on the wire,
//! so [`MgbaWorld`] derives them inside each step by co-simulating the
//! opponent (the [`Shadow`](crate::shadow::Shadow)) — for *both* confirmed
//! settles and speculative ticks, driven by the engine's predicted-then-confirmed
//! remote joyflags. Because the packet is always shadow-derived (never faked),
//! a speculation whose predicted joyflags matched the real ones is byte-exact
//! and the engine can promote it with no re-simulation; only a genuine
//! misprediction triggers a [`load`]+re-step rollback of both cores.
//!
//! [`MgbaWorld`] is the single [`getgud::World`] implementation: it pins the
//! engine's type axes — [`MgbaState`] (the primary + shadow snapshots and our
//! in-flight outgoing packet) and [`PartialInput`] (joyflags) — wraps the single
//! [`Stepper`](crate::stepper::Stepper) core, owns the shadow, and predicts the
//! remote *joyflags* (held A/B) from which the packet falls out of the shadow
//! co-sim.
//!
//! The chosen display state is loaded into the live core — and the time-sync
//! skew turned into a frame-rate target via [`Throttler`](super::throttler::Throttler)
//! — by [`Round`](super::Round), not here.
//!
//! [`load`]: getgud::World::load

use std::sync::{Arc, Mutex as SyncMutex};

use crate::input::{Input, PartialInput};

/// The engine's opaque checkpoint state: the primary stepper's mgba save state,
/// the shadow's snapshot (so a rollback rewinds the opponent co-sim in lockstep),
/// our own outgoing link-cable packet at that tick (needed to continue the
/// exchange on resume), and the tick the bundle is poised at. The engine treats
/// this as a blob; [`MgbaWorld`] reads `tick` to decide whether a `load` is a
/// real rewind or a no-op resume.
pub struct MgbaState {
    pub primary: Box<mgba::state::State>,
    pub outgoing: Vec<u8>,
    pub shadow_snapshot: crate::shadow::ShadowSnapshot,
    pub tick: u32,
}

/// Per-tick remote-packet resolver handed to the stepper.
type Resolver = Box<dyn FnMut(u32, (Input, PartialInput)) -> anyhow::Result<Vec<u8>> + Send>;

/// The single [`getgud::World`] implementation over the per-frame [`Stepper`]
/// core plus the shadow. Pins the engine's type axes ([`MgbaState`] /
/// [`PartialInput`]) and drives the simulation: every [`step`](getgud::World::step)
/// co-simulates the opponent for that tick (real packet from real-or-predicted
/// remote joyflags) and leaves both cores parked at the resulting boundary;
/// [`save`](getgud::World::save) then snapshots them on demand. Deferring the save
/// means a rollback that re-steps N ticks only snapshots the final one.
/// [`load`](getgud::World::load) rewinds both cores to a saved bundle before a
/// rollback re-sim — but is a no-op when the cores are already parked at that
/// tick, so steady-state settles stay forward-only.
///
/// [`Stepper`]: crate::stepper::Stepper
pub struct MgbaWorld {
    pub stepper: crate::stepper::Stepper,
    pub shadow: Arc<SyncMutex<crate::shadow::Shadow>>,
    /// This side's outgoing link packet at the parked tick — seeds the next
    /// step's link exchange, and is the `outgoing` of a [`save`](getgud::World::save)
    /// taken here. (The parked tick itself is owned by the [`Stepper`].)
    pub last_outgoing: Vec<u8>,
    pub replay_writer: Arc<SyncMutex<Option<crate::replay::Writer>>>,
    pub local_player_index: u8,
}

impl getgud::World for MgbaWorld {
    /// Joyflags — what's queued and what crosses the wire.
    type Input = PartialInput;
    type State = MgbaState;
    type Error = anyhow::Error;

    fn step(&mut self, input: (PartialInput, PartialInput)) -> anyhow::Result<getgud::RoundState> {
        // Co-simulate the opponent for this tick: the resolver runs the shadow
        // forward over the (real or predicted) remote joyflags to derive the
        // remote packet. The shadow advances in lockstep with the stepper and is
        // rewound by `load`, so this is identical whether the tick is a confirmed
        // settle or a speculative one.
        let resolver: Resolver = {
            let shadow = self.shadow.clone();
            Box::new(move |tick, ip| shadow.lock().unwrap().apply_input(tick, ip))
        };
        let last_outgoing = self.last_outgoing.clone();
        let result = self.stepper.step(input, &last_outgoing, resolver)?;

        // Both cores are now parked at the boundary (the stepper advanced its own
        // parked tick); record the outgoing packet, but don't snapshot — `save`
        // does that on demand, so a re-stepped rollback tail doesn't pay a
        // save_state per intermediate tick.
        self.last_outgoing = result.boundary.packet;

        // The per-game round-end traps fire while running the round-ending tick's
        // body, so the step that reports a round result marks the boundary after
        // which input pairs are no longer part of the recorded round. The state
        // itself is still valid (the post-round-end animation), and the engine
        // keeps simulating it so the live core can reach the end.
        Ok(if result.round_result.is_some() {
            getgud::RoundState::Ended
        } else {
            getgud::RoundState::Ongoing
        })
    }

    fn save(&mut self) -> anyhow::Result<MgbaState> {
        // Snapshot both cores where the last `step` parked them. The stepper halts
        // the primary exactly at the boundary (so this is byte-identical to a save
        // taken inside the capture trap), and the shadow is parked at the same tick
        // because `step` co-simulated it forward and nothing has advanced it since.
        let (primary, tick) = self.stepper.save()?;
        Ok(MgbaState {
            primary,
            outgoing: self.last_outgoing.clone(),
            shadow_snapshot: self.shadow.lock().unwrap().save_state()?,
            tick,
        })
    }

    fn load(&mut self, state: &MgbaState) -> anyhow::Result<()> {
        // `restore` no-ops (returns false) when the stepper is already parked at
        // `state.tick` — either no speculation moved the cores since this tick
        // settled, or every speculation up to it was promoted. By the lockstep
        // invariant the shadow and `last_outgoing` already hold `state` too, so
        // skip those reloads as well; this keeps steady-state settles
        // forward-only (no `load_state` per frame).
        if self.stepper.restore(&state.primary, state.tick)? {
            self.shadow.lock().unwrap().load_state(&state.shadow_snapshot)?;
            self.last_outgoing = state.outgoing.clone();
        }
        Ok(())
    }

    fn predict(&self, last_remote: &PartialInput) -> PartialInput {
        const HELD_KEYS: u16 = mgba::input::keys::A as u16 | mgba::input::keys::B as u16;
        PartialInput {
            joyflags: last_remote.joyflags & HELD_KEYS,
        }
    }

    fn log(&mut self, pair: &(PartialInput, PartialInput)) {
        if let Some(writer) = self.replay_writer.lock().unwrap().as_mut() {
            writer.write_input(self.local_player_index, pair).expect("write input");
        }
    }
}
