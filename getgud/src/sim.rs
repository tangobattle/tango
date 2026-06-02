use crate::world::{Snapshot, World};

/// The result of one [`Simulator::simulate`] call.
pub struct SimResult<W: World> {
    /// The world advanced to `base.tick + inputs.len() - 1`.
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
pub trait Simulator<W: World>: Send {
    /// Simulate forward from `base`.
    ///
    /// Contract:
    /// - Apply the **first `inputs.len() - 1`** pairs, advancing one tick each.
    /// - The **last** pair is *peeked* — it is the input sampled **at** the
    ///   returned snapshot's tick, not yet integrated. The session re-supplies
    ///   it as `inputs[0]` of the next segment, so do **not** commit it twice.
    /// - Return a snapshot whose `tick == base.tick + inputs.len() - 1`.
    ///
    /// `speculative` is `true` for the disposable display tail and `false` for
    /// authoritative commits — use it to skip work that only matters for
    /// confirmed state (audio, particles, observer-visible side effects).
    fn simulate(
        &mut self,
        base: &Snapshot<W>,
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
pub trait CommitObserver<W: World>: Send {
    /// Called when `pair` is confirmed at `tick`.
    fn on_commit(&mut self, tick: u32, pair: &(W::Input, W::Input));
}
