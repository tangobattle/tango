//! `getgud` — a transport-, input-, and state-agnostic rollback netcode
//! engine.
//!
//! The engine knows nothing about emulators, link cables, networking, or
//! rounds. It owns the rollback *algebra* for a single session: a paired
//! local/remote input queue, one settled checkpoint that advances over
//! confirmed inputs, a throwaway speculative tail rendered ahead of the commit
//! frontier, and the GGPO-style frame-advantage time-sync that keeps two peers'
//! clocks together.
//!
//! It is fully synchronous and pulls in no async runtime. The host drives it:
//! it sends each local input over the wire (attaching
//! [`Session::local_frame_advantage`]), feeds received remote inputs in via
//! [`Session::add_remote_input`], and calls [`Session::advance`] once per
//! displayed frame. Anything spanning more than one session — round
//! boundaries, match lifecycle, the receive loop — lives in the host.
//!
//! It's plain rollback over an opaque [`World::State`] and [`World::Input`]:
//! confirmed inputs are settled into the checkpoint, unconfirmed ones are
//! guessed and re-simulated. A host whose remote input isn't fully known from
//! the wire (e.g. a link-cable game that derives the opponent's per-tick data
//! by co-simulating it) hides that *inside* its [`Simulator`] — the engine
//! never sees it; it just passes `speculative` so the simulator knows whether
//! that side-effectful work should run.
//!
//! Everything game-specific is supplied through three seams:
//!
//! - [`Simulator`]: re-simulate a window of inputs from a checkpoint.
//! - [`Predictor`]: guess an unknown remote input from the last known one.
//! - [`Presenter`]: where the chosen display state and frame-rate target go.
//!
//! The type axes the engine is generic over live on [`World`].

mod error;
mod input;
mod present;
mod session;
mod sim;
mod throttler;
mod world;

pub use error::EngineError;
pub use input::{Pair, PairQueue};
pub use present::Presenter;
pub use session::{Session, SessionParams};
pub use sim::{CommitObserver, Predictor, SimResult, Simulator};
pub use world::{Snapshot, World};
