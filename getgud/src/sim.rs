use crate::world::World;

/// Advances game state one tick at a time, with an explicit reload for rollback.
///
/// The session drives this in three patterns:
///
/// * **extend** — call [`step`](Simulator::step) alone to advance one tick from
///   wherever the simulator is parked (the speculation frontier).
/// * **rollback** — call [`restore`](Simulator::restore) to reload a saved
///   state, then [`step`] once per tick to re-simulate forward with the
///   corrected inputs.
/// * **promote** — neither: a correct prediction needs no simulation, so the
///   session simply reuses the snapshot it already captured.
///
/// The simulator is parked at a tick at all times. [`restore`] sets the park to
/// the restored state's tick; each [`step`] advances it by one. The same state +
/// input must always produce the same output: rollback prediction only works if
/// the simulation is deterministic.
pub trait Simulator<W: World>: Send {
    /// Reload the simulator from `state`, positioning it to [`step`] that
    /// state's tick next. Used to rewind before re-simulating a mispredicted
    /// tail.
    fn restore(&mut self, state: &W::State) -> Result<(), W::Error>;

    /// Advance exactly one tick from the currently parked position by applying
    /// the `(local, remote)` `input` pair.
    ///
    /// Returns the resulting snapshot and whether the round *ended* on this step
    /// (the round-ending tick's body ran). The session keeps simulating **past** a
    /// round end — the post-end frames are still real state, and the host's
    /// presentation typically only detects the end a tick or two later — so it
    /// does not stop on `true`; it only stops committing input pairs to
    /// [`log`](Simulator::log) from that tick on.
    fn step(&mut self, input: (W::Input, W::Input)) -> Result<(W::State, bool), W::Error>;

    /// Return the predicted remote input given the last confirmed remote input.
    fn predict(&self, last_remote: &W::Input) -> W::Input;

    /// Record a single confirmed `(local, remote)` input pair.
    fn log(&mut self, pair: &(W::Input, W::Input));
}
