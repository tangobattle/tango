use crate::world::World;

/// The outcome of a single [`Simulator::simulate`] call.
pub struct SimResult<W: World> {
    /// The new state after applying every input pair that was passed in.
    pub state: W::State,

    /// How many of the supplied input pairs the simulator considers *committed*
    /// (i.e. final and safe to log / surface to gameplay systems).
    ///
    /// During an authoritative settle this is normally equal to the number of
    /// inputs handed in. The session uses it to decide how many leading input
    /// pairs to forward to the [`Logger`]; only the first `committed` pairs are
    /// logged. Values larger than the input count are clamped.
    pub committed: usize,
}

/// Advances game state by applying confirmed or predicted input pairs.
///
/// This is the heart of the game-specific simulation and the one trait every
/// game must implement with real logic. The session calls it in two modes:
///
/// * **Authoritative ("settle")** — `speculative == false`. The inputs are fully
///   confirmed `(local, remote)` pairs. The resulting state becomes the new
///   trusted baseline and the leading pairs are logged.
/// * **Speculative ("tail")** — `speculative == true`. The leading pairs are
///   confirmed but the trailing remote inputs are *predictions* (see
///   [`Predictor`]). The result is shown to the player this frame and thrown
///   away next frame, so it should avoid irreversible side effects.
///
/// The same input must always produce the same output: rollback prediction only
/// works if the simulation is deterministic.
pub trait Simulator<W: World>: Send {
    /// Apply `inputs` on top of `base` and return the resulting state.
    ///
    /// * `base` — the state at `base_tick` to simulate forward from.
    /// * `base_tick` — the tick `base` corresponds to (the first pair in
    ///   `inputs` advances the world from `base_tick` to `base_tick + 1`).
    /// * `inputs` — `(local, remote)` input pairs, one per tick to advance.
    /// * `speculative` — `true` when some trailing remote inputs are predicted
    ///   and the result is a throwaway presentation frame; `false` for an
    ///   authoritative advance of the trusted state.
    fn simulate(
        &mut self,
        base: &W::State,
        base_tick: u32,
        inputs: Vec<(W::Input, W::Input)>,
        speculative: bool,
    ) -> Result<SimResult<W>, W::Error>;
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
