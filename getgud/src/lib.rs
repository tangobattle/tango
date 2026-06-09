//! `getgud` is a small, dependency-free core for **rollback netcode** in
//! two-player deterministic games.
//!
//! Each peer runs its own [`Session`]. You feed it the local player's input
//! every tick and the remote player's inputs as they arrive over the network.
//! The session confirms ticks for which both inputs are known, *predicts* the
//! remote inputs that haven't arrived yet so it can present a responsive frame,
//! and transparently corrects those predictions once the real inputs land. It
//! also produces a clock-[`skew`](Session::skew) signal so the two peers can keep
//! their simulations aligned.
//!
//! The crate is generic over your game and contains no game logic itself. You
//! implement a single trait, [`World`], on the type that owns your live
//! simulation:
//!
//! | Member                            | Responsibility                                   |
//! |-----------------------------------|--------------------------------------------------|
//! | [`World`] `Input`/`State`/`Error` | Names your input, snapshot, and error types.     |
//! | [`step`](World::step)             | Advances the live simulation one tick by an input pair, reporting round end (deterministically). |
//! | [`save`](World::save)             | Snapshots the live simulation at the current tick. |
//! | [`load`](World::load)             | Restores the live simulation to a snapshot, to rewind before a rollback. |
//! | [`predict`](World::predict)       | Guesses the remote's next input from their last one. |
//! | [`log`](World::log)               | Receives confirmed input pairs.                  |
//!
//! # Model
//!
//! * **Frontier** — the newest local tick; advances once per [`Session::advance`].
//! * **Present delay** — how many ticks behind the frontier you present. Larger
//!   means less prediction but more input latency; smaller is snappier but
//!   speculates further ahead. Tunable at runtime.
//! * **Settled state** — the authoritative state built only from confirmed
//!   `(local, remote)` input pairs. Inputs are logged as they settle.
//! * **Speculative tail** — when the presented tick runs past the confirmed
//!   region, the session steps forward from the settled state using real local
//!   inputs and predicted remote inputs, saving each snapshot into a rolling
//!   buffer. When the real remote inputs arrive, the snapshots whose prediction
//!   held are promoted to settled with no re-simulation; only a mispredicted tail
//!   is rolled back and re-stepped.
//!
//! # Per-tick loop
//!
//! ```text
//! loop each tick:
//!     while packet arrived:  session.add_remote_input(remote_input, their_advantage)
//!     adjust_clock(session.skew());   // stall a frame when running ahead
//!     let frame = session.advance(local_input)?;
//!     render(frame.state);
//! ```
//!
//! # Example
//!
//! A toy world whose state is a single integer that each player's input nudges.
//!
//! ```
//! use getgud::{RoundState, Session, SessionParams, World};
//!
//! // A world whose live state is a single integer that each player's input nudges.
//! struct Counter { total: i64 }
//!
//! impl World for Counter {
//!     type Input = i64;
//!     type State = i64;   // the snapshot is just the running total
//!     type Error = std::convert::Infallible;
//!
//!     // Fold each (local, remote) pair into the running total.
//!     fn step(&mut self, input: (i64, i64)) -> Result<RoundState, std::convert::Infallible> {
//!         let (local, remote) = input;
//!         self.total += local + remote;
//!         Ok(RoundState::Ongoing) // this toy round never ends
//!     }
//!     // Snapshot / restore the running total.
//!     fn save(&mut self) -> Result<i64, std::convert::Infallible> { Ok(self.total) }
//!     fn load(&mut self, state: &i64) -> Result<(), std::convert::Infallible> {
//!         self.total = *state;
//!         Ok(())
//!     }
//!     // Predict that the remote keeps repeating its last input.
//!     fn predict(&self, last_remote: &i64) -> i64 { *last_remote }
//!     // This toy world doesn't record confirmed pairs.
//!     fn log(&mut self, _pair: &(i64, i64)) {}
//! }
//!
//! let mut session = Session::<Counter>::new(SessionParams {
//!     present_delay: 2,
//!     initial_remote: 0,
//!     initial_state: 0,
//!     world: Counter { total: 0 },
//! });
//!
//! // Drive ten ticks. Remote inputs arrive two frames late, so the session
//! // must speculate to present the latest frame — and correct itself later.
//! let mut pending: Vec<i64> = Vec::new();
//! for tick in 0..10 {
//!     // A packet from two ticks ago becomes available now.
//!     if tick >= 2 {
//!         session.add_remote_input(pending.remove(0), session.local_tick_advantage());
//!     }
//!     pending.push(1); // the remote input we'll deliver later
//!
//!     // `skew` drives clock sync; read it before `advance`, which enqueues
//!     // this tick's local input.
//!     let skew = session.skew();
//!     let frame = session.advance(1).unwrap();
//!     // `frame.state` is what to render.
//!     let _ = (skew, frame.tick, frame.state, frame.input);
//! }
//!
//! assert_eq!(session.local_frontier(), 10);
//! ```

mod input;
mod session;
mod world;

pub use session::{Frame, Session, SessionParams};
pub use world::{RoundState, World};
