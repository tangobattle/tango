use crate::world::World;

/// The result of one [`Simulator::simulate`] call.
pub struct SimResult<W: World> {
    /// The world advanced one tick per input pair. Per the state invariant it is
    /// *poised at the start of* `base_tick + inputs.len()` — integrated through
    /// the tick before, with that tick's input not yet sampled. The simulator
    /// doesn't report the tick: it's `base_tick + inputs.len()` by construction,
    /// and the session tracks it.
    pub state: W::State,
    /// How many leading pairs of `inputs` the world actually consumed before
    /// reaching a terminal state (e.g. a round ending). Equal to `inputs.len()`
    /// while the world is live; smaller once it terminates partway through the
    /// batch, since a simulator may overshoot the end by a few ticks. The
    /// session reports only these leading pairs to the logger, so a replay isn't
    /// recorded into the overshoot.
    pub committed: usize,
}

/// Advances your world. Supplied to the [`Session`](crate::Session) as a boxed
/// trait object and called for both authoritative commits and throwaway tails.
///
/// **State invariant.** A [`State`](World::State) at tick `T` is the world
/// *poised at the start of* `T`: integrated through `T - 1`, with `T`'s input
/// not yet sampled. The start-of-`T` local input rides out separately on the
/// [`Frame`](crate::Frame) (as [`local_input`](crate::Frame::local_input)), so a
/// consumer that must bake the boundary input onto the displayed state — e.g.
/// priming an input register a resume will read — applies it itself rather than
/// the simulator baking it into the opaque state.
pub trait Simulator<W: World>: Send {
    /// Advance `base` (the world at `base_tick`) by every pair in `inputs`, one
    /// tick per pair, and return the state poised at the start of the resulting
    /// tick.
    ///
    /// Contract:
    /// - Apply **all** of `inputs`, advancing one tick per pair.
    /// - Return the state at `base_tick + inputs.len()`. The tick itself isn't
    ///   returned — it's implied by the count, and the session owns it.
    ///
    /// The input that lands **at** the resulting tick is not passed in and not
    /// stepped here: the session hands it back as `inputs[0]` of the next call
    /// (where it *is* stepped, so it counts exactly once) and surfaces it on the
    /// current [`Frame`](crate::Frame) for the consumer to prime onto the display.
    ///
    /// `speculative` is `true` for the disposable display tail and `false` for
    /// authoritative commits — use it to skip work that only matters for
    /// confirmed state (audio, particles, observer-visible side effects).
    fn simulate(
        &mut self,
        base: &W::State,
        base_tick: u32,
        inputs: Vec<(W::Input, W::Input)>,
        speculative: bool,
    ) -> Result<SimResult<W>, W::Error>;
}

/// Guesses a remote input for the speculative tail. The session holds the guess
/// constant across the whole tail and replaces it as soon as real remote inputs
/// arrive. Cloning `last_remote` ("the peer keeps doing what it last did") is
/// the usual, hard-to-beat strategy.
pub trait Predictor<W: World>: Send + Sync {
    /// Predict the remote input that follows `last_remote`.
    fn predict(&self, last_remote: &W::Input) -> W::Input;
}

/// Optional hook fired once per confirmed input pair as it commits, in tick
/// order. The natural place for replay recording, rollback metrics, or desync
/// hashing. Predictions are never reported — only confirmed history.
pub trait Logger<W: World>: Send {
    /// Called when `pair` is confirmed.
    fn log(&mut self, pair: &(W::Input, W::Input));
}

/// Logger that does nothing.
pub struct NullLogger;

impl<W: World> Logger<W> for NullLogger {
    fn log(&mut self, _pair: &(<W as World>::Input, <W as World>::Input)) {}
}
