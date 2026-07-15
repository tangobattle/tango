/// Binds the concrete types a game provides to the netcode core **and** drives
/// the simulation.
///
/// A `World` names the three associated types the rest of the crate is generic
/// over (`Input`, `State`, `Error`) and supplies the five methods that drive the
/// game: [`step`](World::step), [`save`](World::save), [`load`](World::load),
/// [`predict`](World::predict), and [`log`](World::log). Implement it once for
/// your game on the type that owns the live simulation and pass it as the `W` type
/// parameter to [`Session`](crate::Session).
///
/// A tick's inputs arrive as the local player's input plus a slice of remote
/// inputs, indexed by *remote slot* (the same index the host passes to
/// [`Session::add_remote_input`](crate::Session::add_remote_input)). The
/// two-player case is simply the one-slot case.
///
/// # Stepping model
///
/// A `World` is a *live* simulation parked at one tick at a time, paired with the
/// ability to snapshot and restore that tick. The session drives it in three
/// patterns:
///
/// * **extend** — [`step`](World::step) one tick from where the world is parked,
///   then [`save`](World::save) the result, to speculate forward.
/// * **rollback** — [`load`](World::load) an earlier snapshot, re-[`step`](World::step)
///   forward with the corrected inputs, and [`save`](World::save) the result.
/// * **promote** — neither: a correct prediction needs no simulation, so the
///   session reuses the snapshot it already saved.
///
/// A rollback re-steps the whole corrected tail and `save`s only its final
/// tick, instead of snapshotting every intermediate one.
///
/// [`load`](World::load) parks the world at the restored snapshot's tick; each
/// [`step`](World::step) moves it one tick further. The same state + inputs must
/// always produce the same next state: rollback prediction only works if the
/// simulation is deterministic.
///
/// # Example
///
/// ```
/// use getgud::World;
///
/// struct MyGame {
///     cells: Vec<i32>,
/// }
///
/// impl World for MyGame {
///     type Input = u8;            // e.g. a bitfield of held buttons
///     type State = Vec<i32>;      // a serializable snapshot of the simulation
///     type Error = std::convert::Infallible;
///
///     fn step(&mut self, _local: &u8, _remotes: &[u8]) -> Result<(), std::convert::Infallible> {
///         Ok(())
///     }
///     fn save(&mut self) -> Result<Vec<i32>, std::convert::Infallible> {
///         Ok(self.cells.clone())
///     }
///     fn load(&mut self, snap: &Vec<i32>) -> Result<(), std::convert::Infallible> {
///         self.cells = snap.clone();
///         Ok(())
///     }
///     fn predict(&self, last_remote: &u8) -> u8 { *last_remote }
///     fn log(&mut self, _local: &u8, _remotes: &[u8]) {}
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

    /// A complete, restorable snapshot of the simulation at a single tick.
    ///
    /// This is the value [`save`](World::save) produces, [`load`](World::load)
    /// restores, and [`Frame`](crate::Frame) hands back for rendering. It must
    /// hold everything needed to deterministically continue the simulation from
    /// its tick.
    type State: Send + 'static;

    /// The error type the driving methods may return.
    ///
    /// Use [`std::convert::Infallible`] if your simulation cannot fail.
    type Error: Send + 'static;

    /// Advance the live simulation exactly one tick from where it is parked by
    /// applying this tick's inputs (`remotes` indexed by remote slot), parking
    /// the world one tick further on.
    fn step(&mut self, local: &Self::Input, remotes: &[Self::Input]) -> Result<(), Self::Error>;

    /// Snapshot the live simulation at the tick it is currently parked at.
    ///
    /// The session keeps these snapshots to present, to promote a correct
    /// prediction without re-simulating, and to [`load`](World::load) when
    /// rewinding.
    fn save(&mut self) -> Result<Self::State, Self::Error>;

    /// Restore the live simulation to a previously saved snapshot, parking it at
    /// that snapshot's tick so the next [`step`](World::step) continues from there.
    /// Used to rewind before re-simulating a mispredicted tail.
    fn load(&mut self, state: &Self::State) -> Result<(), Self::Error>;

    /// Take back ownership of a snapshot the session is discarding — an old
    /// settled state displaced by a promotion, or a speculative tail thrown
    /// away by a rollback. Purely an allocation-reuse hook: implementations
    /// with large snapshots can pool the buffers and hand them back out from
    /// [`save`](World::save) instead of allocating fresh ones every tick. The
    /// default just drops the state.
    fn recycle(&mut self, state: Self::State) {
        let _ = state;
    }

    /// Return the predicted next input for a remote peer given that peer's
    /// last known input. Applied independently per remote slot — and only to
    /// the slots whose real input hasn't arrived yet; a remote input that is
    /// already buffered is used as-is.
    fn predict(&self, last_remote: &Self::Input) -> Self::Input;

    /// Record a single confirmed input row (`remotes` indexed by remote slot).
    fn log(&mut self, local: &Self::Input, remotes: &[Self::Input]);
}
