//! `getgud` is a small, dependency-free core for **rollback netcode** in
//! two-player deterministic games.
//!
//! Each peer runs its own [`Session`]. You feed it the local player's input
//! every tick and the remote player's inputs as they arrive over the network.
//! The session confirms ticks for which both inputs are known, *predicts* the
//! remote inputs that haven't arrived yet so it can present a responsive frame,
//! and transparently corrects those predictions once the real inputs land. It
//! also produces a clock-[`skew`](Frame::skew) signal so the two peers can keep
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
//!     let frame = session.advance(local_input)?;
//!     render(frame.state);
//!     adjust_clock(frame.skew);   // stall a frame when running ahead
//! ```
//!
//! # Example
//!
//! A toy world whose state is a single integer that each player's input nudges.
//!
//! ```
//! use std::sync::Arc;
//! use getgud::{
//!     NullLogger, Predictor, Session, SessionParams, SimResult, Simulator, World,
//! };
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
//! struct Sim;
//! impl Simulator<Counter> for Sim {
//!     fn simulate(
//!         &mut self,
//!         base: &i64,
//!         _base_tick: u32,
//!         inputs: Vec<(i64, i64)>,
//!         _speculative: bool,
//!     ) -> Result<SimResult<Counter>, std::convert::Infallible> {
//!         let mut state = *base;
//!         let committed = inputs.len();
//!         for (local, remote) in inputs {
//!             state += local + remote;
//!         }
//!         Ok(SimResult { state, committed })
//!     }
//! }
//!
//! // 3. Predict that the remote keeps repeating its last input.
//! struct Repeat;
//! impl Predictor<Counter> for Repeat {
//!     fn predict(&self, last_remote: &i64) -> i64 { *last_remote }
//! }
//!
//! let mut session = Session::<Counter>::new(SessionParams {
//!     present_delay: 2,
//!     initial_remote: 0,
//!     initial_state: 0,
//!     simulator: Box::new(Sim),
//!     predictor: Arc::new(Repeat),
//!     logger: Box::new(NullLogger),
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
//!     let frame = session.advance(1).unwrap();
//!     // `frame.state` is what to render; `frame.skew` drives clock sync.
//!     let _ = (frame.tick, frame.skew, frame.state, frame.local_input);
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
pub use sim::{Logger, NullLogger, Predictor, SimResult, Simulator};
pub use world::World;
