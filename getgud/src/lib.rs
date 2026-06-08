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
//! provide four things:
//!
//! | You implement        | Responsibility                                            |
//! |----------------------|-----------------------------------------------------------|
//! | [`World`]            | Names your `Input`, `State`, and `Error` types.           |
//! | [`Simulator`]        | Advances `State` by applying input pairs (deterministically). |
//! | [`Predictor`]        | Guesses the remote's next input from their last one.      |
//! | [`Logger`] *(opt.)*  | Receives confirmed input pairs; use [`NullLogger`] to skip. |
//!
//! # Model
//!
//! * **Frontier** — the newest local tick; advances once per [`Session::advance`].
//! * **Present delay** — how many ticks behind the frontier you present. Larger
//!   means less prediction but more input latency; smaller is snappier but
//!   speculates further. Tunable at runtime.
//! * **Settled state** — the authoritative state built only from confirmed
//!   `(local, remote)` input pairs. Inputs are logged as they settle.
//! * **Speculative tail** — when the presented tick runs past the confirmed
//!   region, the session simulates forward from the settled state using real
//!   local inputs and predicted remote inputs. It is rebuilt from scratch every
//!   frame, so a wrong prediction simply disappears once the true input settles.
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
//! use getgud::{Session, SessionParams, Simulator, World};
//!
//! // 1. Describe the game's types.
//! struct Counter;
//! impl World for Counter {
//!     type Input = i64;
//!     type State = i64;
//!     type Error = std::convert::Infallible;
//! }
//!
//! // 2. The simulation: fold each (local, remote) pair into the running total.
//! //    It is parked at a state; `restore` reloads it, `step` advances it.
//! struct Sim { state: i64 }
//! impl Simulator<Counter> for Sim {
//!     fn restore(&mut self, state: &i64) -> Result<(), std::convert::Infallible> {
//!         self.state = *state;
//!         Ok(())
//!     }
//!     fn step(
//!         &mut self,
//!         input: (i64, i64),
//!     ) -> Result<(i64, bool), std::convert::Infallible> {
//!         let (local, remote) = input;
//!         self.state += local + remote;
//!         Ok((self.state, false)) // never ends
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
//!     simulator: Box::new(Sim { state: 0 }),
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
//! assert_eq!(session.frontier(), 10);
//! ```

mod input;
mod session;
mod sim;
mod world;

pub use input::Queue;
pub use session::{Frame, Session, SessionParams};
pub use sim::Simulator;
pub use world::World;
