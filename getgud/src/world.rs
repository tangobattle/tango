/// Your game's type contract — what the engine simulates over.
///
/// Implement this on a marker type and wire its three associated types to your
/// game. Everything else in the crate is generic over `W: World`.
pub trait World: Sized + 'static {
    /// One participant's input for one tick. The session pairs the local and
    /// remote `Input`s into a `(local, remote)` tuple; a `Predictor` clones one
    /// to guess the remote's next input, so it must be `Clone`.
    type Input: Clone + Send + 'static;
    /// A complete, restorable world state. The [`Simulator`](crate::Simulator)
    /// resumes from one of these, so it must capture *everything* the simulation
    /// reads — anything omitted will desync on rollback. The engine pairs the
    /// authoritative settled state with the tick it tracks itself, rather than
    /// trusting the simulator to report one back.
    type State: Send + 'static;
    /// The error a simulation step can fail with, surfaced out of
    /// [`Session::advance`](crate::Session::advance).
    type Error: Send + 'static;
}
