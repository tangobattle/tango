/// Binds the concrete types a game provides to the netcode core **and** drives
/// the simulation.
///
/// A `World` names the three associated types the rest of the crate is generic
/// over (`Input`, `State`, `Error`) and supplies the four methods that advance
/// the game: [`restore`](World::restore), [`step`](World::step),
/// [`predict`](World::predict), and [`log`](World::log). Implement it once for
/// your game on the type that owns the simulation and pass it as the `W` type
/// parameter to [`Session`](crate::Session).
///
/// # Stepping model
///
/// The session drives the stepping methods in three patterns:
///
/// * **extend** — call [`step`](World::step) alone to advance one tick from
///   wherever the world is parked (the speculation frontier).
/// * **rollback** — call [`restore`](World::restore) to reload a saved state,
///   then [`step`](World::step) once per tick to re-simulate forward with the
///   corrected inputs.
/// * **promote** — neither: a correct prediction needs no simulation, so the
///   session simply reuses the snapshot it already captured.
///
/// The world is parked at a tick at all times. [`restore`](World::restore) sets
/// the park to the restored state's tick; each [`step`](World::step) advances it
/// by one. The same state + input must always produce the same output: rollback
/// prediction only works if the simulation is deterministic.
///
/// # Example
///
/// ```
/// use getgud::World;
///
/// struct MyGame {
///     state: Vec<i32>,
/// }
///
/// impl World for MyGame {
///     type Input = u8;            // e.g. a bitfield of held buttons
///     type State = Vec<i32>;      // your full, serializable game state
///     type Error = std::convert::Infallible;
///
///     fn restore(&mut self, state: &Vec<i32>) -> Result<(), std::convert::Infallible> {
///         self.state = state.clone();
///         Ok(())
///     }
///     fn step(&mut self, _input: (u8, u8)) -> Result<(Vec<i32>, bool), std::convert::Infallible> {
///         Ok((self.state.clone(), false))
///     }
///     fn predict(&self, last_remote: &u8) -> u8 { *last_remote }
///     fn log(&mut self, _pair: &(u8, u8)) {}
/// }
/// ```
pub trait World {
    /// One player's input for a single tick.
    ///
    /// Must be cheap to [`Clone`] — the session clones inputs while matching
    /// local and remote streams and while building speculative tails — and
    /// [`PartialEq`], so a confirmed remote input can be checked against the one
    /// that was predicted speculatively to decide promote-vs-rollback.
    type Input: Clone + PartialEq + Send + 'static;

    /// The complete simulation state at a single tick.
    ///
    /// This is the value [`step`](World::step) advances and that
    /// [`Frame`](crate::Frame) hands back for rendering. It must hold everything
    /// needed to deterministically continue the simulation from a given tick.
    type State: Send + 'static;

    /// The error type the stepping methods may return.
    ///
    /// Use [`std::convert::Infallible`] if your simulation cannot fail.
    type Error: Send + 'static;

    /// Reload the world from `state`, positioning it to [`step`](World::step)
    /// that state's tick next. Used to rewind before re-simulating a
    /// mispredicted tail.
    fn restore(&mut self, state: &Self::State) -> Result<(), Self::Error>;

    /// Advance exactly one tick from the currently parked position by applying
    /// the `(local, remote)` `input` pair.
    ///
    /// Returns the resulting snapshot and whether the round *ended* on this step
    /// (the round-ending tick's body ran). The session keeps simulating **past** a
    /// round end — the post-end frames are still real state, and the host's
    /// presentation typically only detects the end a tick or two later — so it
    /// does not stop on `true`; it only stops committing input pairs to
    /// [`log`](World::log) from that tick on.
    fn step(&mut self, input: (Self::Input, Self::Input)) -> Result<(Self::State, bool), Self::Error>;

    /// Return the predicted remote input given the last confirmed remote input.
    fn predict(&self, last_remote: &Self::Input) -> Self::Input;

    /// Record a single confirmed `(local, remote)` input pair.
    fn log(&mut self, pair: &(Self::Input, Self::Input));
}
