use crate::input::Pair;
use crate::world::{Snapshot, World};

/// The result of one [`Simulator::simulate`] run.
pub struct SimResult<W: World> {
    /// State captured at the run's last input — becomes the next checkpoint
    /// and, in the settled regime, the displayed state.
    pub snapshot: Snapshot<W>,
    /// Exclusive upper bound on which committed ticks the engine reports to the
    /// [`CommitObserver`]. When a run hits a terminal state this is that tick,
    /// so commits at or past it — the few the simulator overshoots the end by —
    /// aren't reported (keeping e.g. a replay from recording past the round).
    /// Purely a recording bound: the engine makes no decision about whether the
    /// bout is over (that's the host's job). `None` = report every commit.
    pub commit_before: Option<u32>,
}

/// The deterministic core of the game: re-simulate a window of inputs from a
/// checkpoint, with save/restore and a single forward run per call.
///
/// The window is inclusive of the capture tick, so the captured snapshot lands
/// at `base.tick + inputs.len() - 1` (the engine never calls this with empty
/// `inputs`).
pub trait Simulator<W: World>: Send {
    /// From `base`, process `inputs` and capture a fresh snapshot.
    ///
    /// `speculative` distinguishes the two regimes for hosts that carry
    /// side-effectful state across runs (e.g. a link-cable game's opponent
    /// co-simulation): a non-speculative (settle) run resolves real remote data
    /// and may advance that state; a speculative run is throwaway and must
    /// leave it untouched (predicting whatever it needs instead). Hosts whose
    /// inputs are fully known from the wire can ignore the flag.
    fn simulate(
        &mut self,
        base: &Snapshot<W>,
        inputs: Vec<Pair<W::Input, W::Input>>,
        speculative: bool,
    ) -> Result<SimResult<W>, W::Error>;
}

/// Guesses an unknown remote input from the last known one. Applied repeatedly
/// across the speculative window, so it must be stable under iteration (a
/// fixpoint, not a drift).
pub trait Predictor<W: World>: Send + Sync {
    fn predict(&self, last_remote: &W::Input) -> W::Input;
}

/// Observes each input pair as it becomes committed (both peers' real inputs
/// known). Used for e.g. recording a replay. Pairs are reported in tick order;
/// pairs at or past a terminal tick are not reported.
pub trait CommitObserver<W: World>: Send {
    fn on_commit(&mut self, tick: u32, pair: &Pair<W::Input, W::Input>);
}
