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
//! remote *joyflags* (repeat-last) from which the packet falls out of the
//! shadow co-sim.
//!
//! The chosen display state is loaded into the live core — and the time-sync
//! skew turned into a frame-rate target via [`Throttler`](super::throttler::Throttler)
//! — by [`Round`](super::Round), not here.
//!
//! [`load`]: getgud::World::load

use std::sync::{Arc, Mutex as SyncMutex};

use crate::input::PartialInput;

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
    /// The opponent co-sim, behind its concurrent driver: each step's trap
    /// hands the worker the tick's input pair and gets the (already-buffered)
    /// remote packet back immediately, while the shadow's own tick runs on
    /// the worker thread overlapping the rest of the primary's tick. The
    /// `save`/`load` paths below go through the worker so they join the
    /// in-flight run first.
    pub shadow: Arc<crate::shadow::Worker>,
    /// This side's outgoing link packet at the parked tick — seeds the next
    /// step's link exchange, and is the `outgoing` of a [`save`](getgud::World::save)
    /// taken here. (The parked tick itself is owned by the [`Stepper`].)
    pub last_outgoing: Vec<u8>,
    pub replay_writer: Arc<SyncMutex<Option<crate::replay::Writer>>>,
    pub local_player_index: u8,
    /// The standing round outcome, shared with the owning
    /// [`Round`](super::Round) so `Match::end_round` can read it when the live
    /// core reaches the round-end screen. Written by [`step`](getgud::World::step)
    /// whenever the per-game round-end traps report one — including on
    /// speculative ticks, so [`load`](getgud::World::load) revokes it again when
    /// a rollback rewinds past the step that reported it (the re-sim decides
    /// afresh whether the round really ended). The stored tick is the reporting
    /// step's *boundary* tick, which is what makes that comparison exact; a
    /// result whose boundary settled or promoted is never revoked.
    pub round_result: Arc<SyncMutex<Option<crate::stepper::RoundResult>>>,
    /// Local/remote-oriented HP per simulated tick, shared with the owning
    /// [`Round`](super::Round) the same way `round_result` is: [`step`]
    /// appends (including on speculative ticks) and [`load`] truncates the
    /// speculative tail on a genuine rewind, so what stands at any moment is
    /// exactly the ticks the current timeline has simulated. `Match::end_round`
    /// drains it into the round's report.
    ///
    /// [`step`]: getgud::World::step
    /// [`load`]: getgud::World::load
    pub hp_series: Arc<SyncMutex<Vec<HpSample>>>,
    /// Spent ~400KB mgba state buffers harvested from snapshots the engine
    /// discards ([`recycle`](getgud::World::recycle)), handed back out by
    /// [`save`](getgud::World::save). In steady state every frame discards one
    /// snapshot bundle and saves one, so the per-frame saves run entirely on
    /// reused buffers instead of round-tripping the page allocator.
    pub state_pool: Vec<Box<std::mem::MaybeUninit<mgba::state::State>>>,
}

/// Cap on pooled state buffers. The engine discards at most a speculation
/// tail's worth in one frame (a rollback); anything past this is genuinely
/// surplus and is returned to the allocator.
const STATE_POOL_CAP: usize = 16;

/// One simulated tick's HP reading, oriented to this side of the match.
/// `tick` is the tick that was simulated (not the boundary it produced), so
/// consecutive samples are dense except for ticks the per-game traps skipped
/// (battle intro, before the unit structs are live).
#[derive(Clone, Copy)]
pub struct HpSample {
    pub tick: u32,
    pub local: u16,
    pub remote: u16,
    /// Whether the custom screen (chip select) was open this tick — false
    /// on games whose traps don't report it.
    pub custom: bool,
    /// Both players' A/B button state this tick (see the `BUTTON_*` bit
    /// constants) — the raw held bits from the tick's confirmed input
    /// pair, from which buster usage events are derived downstream.
    pub buttons: u8,
    /// `[local, remote]` loaded chip ids this tick ([`NO_CHIP`] = none or
    /// not reported) — chip-use events are their departures downstream.
    pub chips: [u16; 2],
}

/// Sentinel for "no chip loaded" in [`HpSample::chips`] — the games' own
/// in-memory sentinel.
pub const NO_CHIP: u16 = 0xffff;

/// Bits of [`HpSample::buttons`].
pub const BUTTON_LOCAL_A: u8 = 1 << 0;
pub const BUTTON_LOCAL_B: u8 = 1 << 1;
pub const BUTTON_REMOTE_A: u8 = 1 << 2;
pub const BUTTON_REMOTE_B: u8 = 1 << 3;

/// GBA KEYINPUT bits for A and B in the joyflags word.
pub(crate) const JOY_A: u16 = 0x0001;
pub(crate) const JOY_B: u16 = 0x0002;

impl getgud::World for MgbaWorld {
    /// Joyflags — what's queued and what crosses the wire.
    type Input = PartialInput;
    type State = MgbaState;
    type Error = anyhow::Error;

    fn step(&mut self, input: (PartialInput, PartialInput)) -> anyhow::Result<getgud::RoundState> {
        let (local_joy, remote_joy) = (input.0.joyflags, input.1.joyflags);
        // Co-simulate the opponent for this tick: the stepper's
        // [`RemotePacketSource`](crate::stepper::RemotePacketSource) — our shared
        // shadow handle, set at construction — runs the shadow forward over the
        // (real or predicted) remote joyflags to derive the remote packet. The
        // shadow advances in lockstep with the stepper and is rewound by `load`,
        // so this is identical whether the tick is a confirmed settle or a
        // speculative one.
        let result = self.stepper.step(input, &self.last_outgoing)?;

        // Both cores are now parked at the boundary (the stepper advanced its own
        // parked tick); record the outgoing packet, but don't snapshot — `save`
        // does that on demand, so a re-stepped rollback tail doesn't pay a
        // save_state per intermediate tick.
        self.last_outgoing = result.boundary.packet;

        // The tick just simulated is the one before the boundary. Samples from
        // a mispredicted speculation get appended here too — `load` truncates
        // them again when the rollback rewinds, and the re-sim re-appends the
        // corrected values.
        if let Some(hp) = result.hp {
            let lpi = self.local_player_index as usize;
            let chips = result.chips.unwrap_or([NO_CHIP; 2]);
            let mut buttons = 0u8;
            buttons |= if local_joy & JOY_A != 0 { BUTTON_LOCAL_A } else { 0 };
            buttons |= if local_joy & JOY_B != 0 { BUTTON_LOCAL_B } else { 0 };
            buttons |= if remote_joy & JOY_A != 0 { BUTTON_REMOTE_A } else { 0 };
            buttons |= if remote_joy & JOY_B != 0 { BUTTON_REMOTE_B } else { 0 };
            self.hp_series.lock().unwrap().push(HpSample {
                tick: result.boundary.tick - 1,
                local: hp[lpi],
                remote: hp[1 - lpi],
                custom: result.custom.unwrap_or(false),
                buttons,
                chips: [chips[lpi], chips[1 - lpi]],
            });
        }

        // The per-game round-end traps fire while running the round-ending tick's
        // body, so the step that reports a round result marks the boundary after
        // which input pairs are no longer part of the recorded round. The state
        // itself is still valid (the post-round-end animation), and the engine
        // keeps simulating it so the live core can reach the end.
        Ok(if let Some(rr) = result.round_result {
            // Stamp the outcome with this step's boundary tick (not the
            // trap-time game tick, whose position relative to the tick
            // increment is ROM-dependent) so `load` can tell exactly whether
            // a rewind discards the reporting step.
            *self.round_result.lock().unwrap() = Some(crate::stepper::RoundResult {
                tick: result.boundary.tick,
                outcome: rr.outcome,
            });
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
        // The shadow's save is queued on its worker first — it runs right after the
        // in-flight tick run, overlapping the primary's save below — and collected
        // after, so the two ~400KB snapshots are taken concurrently.
        let buf = self.state_pool.pop().unwrap_or_else(mgba::state::State::new_uninit);
        let pending_shadow = self.shadow.begin_save_state_reusing(buf)?;
        let buf = self.state_pool.pop().unwrap_or_else(mgba::state::State::new_uninit);
        let (primary, tick) = self.stepper.save_reusing(buf)?;
        // Surface a failed shadow run before consuming the snapshot it would
        // have corrupted.
        self.shadow.join_pending()?;
        Ok(MgbaState {
            primary,
            outgoing: self.last_outgoing.clone(),
            shadow_snapshot: pending_shadow.wait()?,
            tick,
        })
    }

    fn recycle(&mut self, state: MgbaState) {
        let MgbaState {
            primary,
            shadow_snapshot,
            ..
        } = state;
        for spent in [primary, shadow_snapshot.mgba_state] {
            if self.state_pool.len() >= STATE_POOL_CAP {
                break;
            }
            self.state_pool.push(mgba::state::State::into_uninit(spent));
        }
    }

    fn load(&mut self, state: &MgbaState) -> anyhow::Result<()> {
        // `restore` no-ops (returns false) when the stepper is already parked at
        // `state.tick` — either no speculation moved the cores since this tick
        // settled, or every speculation up to it was promoted. By the lockstep
        // invariant the shadow and `last_outgoing` already hold `state` too, so
        // skip those reloads as well; this keeps steady-state settles
        // forward-only (no `load_state` per frame).
        if self.stepper.restore(&state.primary, state.tick)? {
            self.shadow.load_state(&state.shadow_snapshot)?;
            self.last_outgoing = state.outgoing.clone();
            // A genuine rewind discards every step past `state.tick` — if the
            // standing round result came from one of them (a speculative KO
            // built on mispredicted remote input), revoke it; the re-sim
            // decides afresh whether the round really ended. A result whose
            // reporting step lies at or before the restore point is settled
            // history and stands.
            let mut round_result = self.round_result.lock().unwrap();
            if round_result.is_some_and(|rr| rr.tick > state.tick) {
                *round_result = None;
            }
            // Same revocation for the HP series: a sample for tick t came from
            // the step that produced boundary t+1, so everything at or past the
            // restore point is speculative history the re-sim will redo.
            let mut hp_series = self.hp_series.lock().unwrap();
            while hp_series.last().is_some_and(|s| s.tick >= state.tick) {
                hp_series.pop();
            }
        }
        Ok(())
    }

    /// Repeat-last: assume the remote keeps holding exactly what they held.
    /// Measured over the replay corpus (see `examples/predictor-eval.rs`) this
    /// roughly third-ed the rollback rate of the old keep-only-A|B mask at
    /// every speculation depth — every button flips less often than it's held,
    /// so predicting any of them released loses.
    fn predict(&self, last_remote: &PartialInput) -> PartialInput {
        last_remote.clone()
    }

    fn log(&mut self, pair: &(PartialInput, PartialInput)) {
        if let Some(writer) = self.replay_writer.lock().unwrap().as_mut() {
            writer.write_input(self.local_player_index, pair).expect("write input");
        }
    }
}
