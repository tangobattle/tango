/// Binds together the concrete types a game provides to the netcode core.
///
/// A `World` is a marker type (it holds no data) whose sole purpose is to name
/// the three associated types the rest of the crate is generic over. Implement
/// it once for your game and pass it as the `W` type parameter to
/// [`Session`](crate::Session), [`Simulator`](crate::Simulator), and friends.
///
/// # Example
///
/// ```
/// use getgud::World;
///
/// struct MyGame;
///
/// impl World for MyGame {
///     type Input = u8;            // e.g. a bitfield of held buttons
///     type State = Vec<i32>;      // your full, serializable game state
///     type Error = std::convert::Infallible;
/// }
/// ```
pub trait World: Sized + 'static {
    /// One player's input for a single tick.
    ///
    /// Must be cheap to [`Clone`] — the session clones inputs while matching
    /// local and remote streams and while building speculative tails.
    type Input: Clone + Send + 'static;

    /// The complete simulation state at a single tick.
    ///
    /// This is the value the [`Simulator`](crate::Simulator) advances and that
    /// [`Frame`](crate::Frame) hands back for rendering. It must hold everything
    /// needed to deterministically continue the simulation from a given tick.
    type State: Send + 'static;

    /// The error type a [`Simulator`](crate::Simulator) may return.
    ///
    /// Use [`std::convert::Infallible`] if your simulation cannot fail.
    type Error: Send + 'static;
}
