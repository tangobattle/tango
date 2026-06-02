/// The type axes the engine is generic over, plus the error type its seams
/// report through.
///
/// A `World` is a marker type — it's never instantiated, it just bundles the
/// associated types so the driver and traits don't carry the parameters each.
/// The engine is plain rollback: it knows nothing about how a tick's input is
/// produced or what a [`State`](World::State) contains — a host whose remote
/// input isn't fully known from the wire (e.g. a link-cable game deriving the
/// opponent's packets) hides that inside its [`Simulator`](crate::Simulator).
pub trait World: Sized + 'static {
    /// One tick's input for a player. The local one is whatever the host puts
    /// on the wire; the remote one is fed in via
    /// [`Session::add_remote_input`](crate::Session::add_remote_input) or
    /// guessed by the [`Predictor`](crate::Predictor).
    type Input: Clone + Send + 'static;
    /// A serializable simulation checkpoint. Opaque to the engine — bundle
    /// whatever a re-sim needs to resume (for a link-cable game, that includes
    /// the in-flight outgoing packet).
    type State: Send + 'static;
    /// Error type the seams (simulator, …) report. An adapter is free to use
    /// `anyhow::Error` or any concrete enum here.
    type Error: Send + 'static;
}

/// A simulation checkpoint captured at `tick`.
pub struct Snapshot<W: World> {
    pub state: W::State,
    /// The tick the simulation is *about to process next* — an exclusive upper
    /// bound on what has already been simulated.
    pub tick: u32,
}
