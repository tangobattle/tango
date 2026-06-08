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
    /// Returns the resulting snapshot, or `None` if the simulated world ended
    /// (e.g. the round finished) during this step — i.e. there is no further
    /// state to advance into. The session stops settling/speculating and stops
    /// logging at the first `None`; the last `Some` snapshot is the terminal
    /// frame to present.
    fn step(&mut self, input: (W::Input, W::Input)) -> Result<Option<W::State>, W::Error>;
}

/// Guesses the remote player's next input from their most recent confirmed one.
///
/// When local simulation runs ahead of the inputs that have arrived over the
/// network, the session fills the gap with predictions so it can present a
/// responsive frame. Mispredictions are corrected automatically: once the real
/// remote input arrives it is settled into the authoritative state, replacing
/// whatever was predicted.
///
/// The simplest useful predictor is "repeat the last input", which assumes the
/// remote player keeps doing what they were doing.
pub trait Predictor<W: World>: Send + Sync {
    /// Return the predicted remote input given the last confirmed remote input.
    fn predict(&self, last_remote: &W::Input) -> W::Input;
}

/// Receives confirmed input pairs as they become final.
///
/// The session calls [`log`](Logger::log) for each `(local, remote)` pair the
/// moment it is settled into the authoritative state, in tick order. This is the
/// hook for recording replays, sending confirmed inputs to a spectator/server,
/// or building a desync-detection trail. Use [`NullLogger`] if you need none of
/// that.
pub trait Logger<W: World>: Send {
    /// Record a single confirmed `(local, remote)` input pair.
    fn log(&mut self, pair: &(W::Input, W::Input));
}

/// A [`Logger`] that discards everything. The default when you don't need logging.
pub struct NullLogger;

impl<W: World> Logger<W> for NullLogger {
    fn log(&mut self, _pair: &(<W as World>::Input, <W as World>::Input)) {}
}
