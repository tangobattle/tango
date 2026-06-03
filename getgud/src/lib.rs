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
//! built purely from confirmed input pairs, and each displayed tick is a
//! *throwaway* re-simulation from that checkpoint forward — predicting the
//! remote's not-yet-received inputs. Confirmed inputs fold into the checkpoint;
//! predictions live only in the disposable tail, so a wrong guess can never
//! corrupt authoritative state.
//!
//! You parameterize the engine over a [`World`] (your game's `Input` / `State`
//! / `Error` types) and supply three behaviors as trait objects:
//!
//! - [`Simulator`] — advance the world by a list of input pairs.
//! - [`Predictor`] — guess a remote input from the last confirmed one.
//! - [`CommitObserver`] — optional; observe confirmed history (e.g. replays).
//!
//! # Driving a session
//!
//! Construct with [`Session::new`] (passing the tick-0 world state as
//! [`SessionParams::initial_state`]), then call [`advance`](Session::advance)
//! once per tick — it advances the wall clock and hands back the [`Frame`] to
//! draw (carrying the time-sync skew to throttle against). Feed remote inputs in
//! as they arrive with [`add_remote_input`](Session::add_remote_input).

mod input;
mod session;
mod sim;
mod world;

pub use input::Queue;
pub use session::{Frame, Session, SessionParams};
pub use sim::{Logger, NullLogger, Predictor, SimResult, Simulator};
pub use world::{Snapshot, World};
