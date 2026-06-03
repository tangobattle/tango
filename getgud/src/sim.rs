use crate::world::{Snapshot, World};

/// The result of one [`Simulator::simulate`] call.
pub struct SimResult<W: World> {
    /// The world advanced to `base.tick + inputs.len()`. Per the snapshot
    /// invariant it is *poised at the start of* that tick — integrated through
    /// the tick before, with `next_input` sampled but not yet stepped.
    pub snapshot: Snapshot<W>,
    /// `Some(end_tick)` once the world has reached a terminal state (e.g. a
    /// round ending) at `end_tick`; the session then stops reporting committed
    /// inputs at or past that tick, so replays aren't recorded into the few
    /// ticks a simulator may overshoot the end by. `None` while the world is
    /// live.
    pub commit_before: Option<u32>,
}

/// Advances your world. Supplied to the [`Session`](crate::Session) as a boxed
/// trait object and called for both authoritative commits and throwaway tails.
///
/// **Snapshot invariant.** A [`Snapshot`] at tick `T` is the world *poised at
/// the start of* `T`: integrated through `T - 1`, with `T`'s input sampled but
/// not yet stepped. Every rule below is a consequence of this one sentence.
pub trait Simulator<W: World>: Send {
    /// Advance `base` by every pair in `inputs`, then sample `next_input` at the
    /// resulting tick without stepping it.
    ///
    /// Contract:
    /// - Apply **all** of `inputs`, advancing one tick per pair.
    /// - Return a snapshot whose `tick == base.tick + inputs.len()`.
    /// - `next_input` is the input that lands **at** that snapshot tick — the
    ///   start-of-`T` input the invariant leaves un-stepped. The session hands
    ///   it straight back as `inputs[0]` of the next call, where it *is* stepped,
    ///   so it counts exactly once. A simulator whose state is a clean inter-tick
    ///   value can ignore it and let that next call do the work; it exists for
    ///   engines that must bake the boundary input into an opaque snapshot up
    ///   front (e.g. priming an input register a resume will read). Passing it
    ///   separately — rather than as `inputs.last()` — is deliberate: there is
    ///   no "the last element is special" rule to get wrong.
    ///
    /// `speculative` is `true` for the disposable display tail and `false` for
    /// authoritative commits — use it to skip work that only matters for
    /// confirmed state (audio, particles, observer-visible side effects).
    fn simulate(
        &mut self,
        base: &Snapshot<W>,
        inputs: Vec<(W::Input, W::Input)>,
        next_input: (W::Input, W::Input),
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
pub trait CommitObserver<W: World>: Send {
    /// Called when `pair` is confirmed at `tick`.
    fn on_commit(&mut self, tick: u32, pair: &(W::Input, W::Input));
}
