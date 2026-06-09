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
/// remote joyflags) and captures a boundary snapshot of both cores, which the
/// next [`save`](getgud::World::save) hands back to the engine.
/// [`load`](getgud::World::load) rewinds both cores to a saved bundle before a
/// rollback re-sim — but is a no-op when the cores are already parked at that
/// tick, so steady-state settles stay forward-only.
///
/// [`Stepper`]: crate::stepper::Stepper
pub struct MgbaWorld {
    pub stepper: crate::stepper::Stepper,
    pub shadow: Arc<SyncMutex<crate::shadow::Shadow>>,
    /// The tick both cores are currently parked at.
    pub parked_tick: u32,
    /// This side's outgoing link packet at the parked tick — seeds the next
    /// step's link exchange.
    pub last_outgoing: Vec<u8>,
    pub replay_writer: Arc<SyncMutex<Option<crate::replay::Writer>>>,
    pub local_player_index: u8,
    /// The boundary snapshot captured by the most recent [`step`](getgud::World::step),
    /// handed back by the next [`save`](getgud::World::save). The stepper captures
    /// the primary snapshot inherently while running the tick, and the shadow
    /// snapshot must be taken at the same boundary (before any later step advances
    /// the shadow), so both are bundled here in `step` rather than re-derived in
    /// `save`.
    pub captured: Option<MgbaState>,
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
        let result = self.stepper.step(input, self.parked_tick, &last_outgoing, resolver)?;
        let shadow_snapshot = self.shadow.lock().unwrap().save_state()?;

        self.parked_tick = result.snapshot.tick;
        self.last_outgoing = result.snapshot.packet.clone();

        // The per-game round-end traps fire while running the round-ending tick's
        // body, so the step that reports a round result marks the boundary after
        // which input pairs are no longer part of the recorded round. The state
        // itself is still valid (the post-round-end animation), and the engine
        // keeps simulating it so the live core can reach the end.
        let round = if result.round_result.is_some() {
            getgud::RoundState::Ended
        } else {
            getgud::RoundState::Ongoing
        };

        // Stash the boundary snapshot of both cores; the next `save` hands it back.
        self.captured = Some(MgbaState {
            primary: result.snapshot.state,
            outgoing: result.snapshot.packet,
            shadow_snapshot,
            tick: result.snapshot.tick,
        });
        Ok(round)
    }

    fn save(&mut self) -> anyhow::Result<MgbaState> {
        // The most recent `step` captured the boundary snapshot of both cores;
        // hand it over. The engine only ever `save`s the tick it just `step`ped,
        // so there is always exactly one waiting.
        self.captured
            .take()
            .ok_or_else(|| anyhow::format_err!("save called without a preceding step"))
    }

    fn load(&mut self, state: &MgbaState) -> anyhow::Result<()> {
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
