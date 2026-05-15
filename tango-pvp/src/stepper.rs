//! Per-frame battle simulation, shared by:
//!
//! - The live PvP fastforwarder ([`Fastforwarder`]) — loads a saved state,
//!   runs forward over a known input window, returns a fresh dirty state.
//! - Replay playback — boots from SRAM, drives the per-game stepper traps
//!   off [`State`] across an entire match's queued rounds.
//!
//! [`InnerState`] holds the per-frame mutable state (input queues, current
//! tick, committed/dirty save snapshots, replay-mode bookkeeping). Per-game
//! stepper traps lock it via [`State::lock_inner`].

mod fastforwarder;
mod state;
mod types;

pub use fastforwarder::{FastforwardResult, Fastforwarder};
pub use state::{InnerState, ReplayCheckpoint, ReplaySnapshot, State};
pub use types::{BattleOutcome, RoundResult};
