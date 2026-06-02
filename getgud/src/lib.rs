//! An engine-agnostic **rollback netcode** core for two participants — a local
//! player and one remote peer.
//!
//! `getgud` owns input matching, confirmed-state checkpointing, speculative
//! re-simulation, and time synchronization. It does no networking, threading,
//! rendering, or timekeeping: you feed it local inputs and the remote inputs
//! that arrive off your transport, and it tells you which world state to draw
//! and the clock skew to throttle against to stay in sync with the peer.
//!
//! # How it fits together
//!
//! Everything is indexed by an integer **tick** (one simulation step). The
//! engine keeps a [`Session`] that holds an authoritative *settled* checkpoint
//! built purely from confirmed input pairs, and each displayed frame is a
//! *throwaway* re-simulation from that checkpoint forward — predicting the
//! remote's not-yet-received inputs. Confirmed inputs fold into the checkpoint;
//! predictions live only in the disposable tail, so a wrong guess can never
//! corrupt authoritative state.
//!
//! You parameterize the engine over a [`World`] (your game's `Input` / `State`
//! / `Error` types) and supply four behaviors as trait objects:
//!
//! - [`Simulator`] — advance the world by a list of input pairs.
//! - [`Predictor`] — guess a remote input from the last confirmed one.
//! - [`Presenter`] — receive the state to draw and the time-sync skew.
//! - [`CommitObserver`] — optional; observe confirmed history (e.g. replays).
//!
//! # Driving a session
//!
//! Construct with [`Session::new`], seed tick 0 with
//! [`set_first_settled_state`](Session::set_first_settled_state), then each
//! frame call [`advance_frontier`](Session::advance_frontier) followed by
//! [`advance`](Session::advance). Feed remote inputs in as they arrive with
//! [`add_remote_input`](Session::add_remote_input). See the crate README for a
//! worked example and the full architecture overview.

mod input;
mod present;
mod session;
mod sim;
mod world;

pub use input::Queue;
pub use present::Presenter;
pub use session::{Session, SessionParams};
pub use sim::{CommitObserver, Predictor, SimResult, Simulator};
pub use world::{Snapshot, World};
