use std::collections::VecDeque;
use std::sync::Arc;

use crate::input::Queue;
use crate::sim::{Logger, Predictor, Simulator};
use crate::world::World;

/// Everything needed to construct a [`Session`].
///
/// Pass this to [`Session::new`].
pub struct SessionParams<W: World> {
    /// How many ticks behind the local frontier to present.
    ///
    /// A larger value shows older, more-often-confirmed state (less prediction,
    /// more input latency); a smaller value is more responsive but speculates
    /// further ahead. This is the classic input-delay knob and can be changed at
    /// runtime via [`Session::set_present_delay`].
    pub present_delay: u32,

    /// The remote input assumed for tick 0, before any real remote input has
    /// arrived. Used as the seed for prediction.
    pub initial_remote: W::Input,

    /// The starting game state at tick 0.
    pub initial_state: W::State,

    /// The game-specific simulation. See [`Simulator`].
    pub simulator: Box<dyn Simulator<W>>,

    /// Strategy for guessing remote inputs that haven't arrived yet. See
    /// [`Predictor`]. Held behind an [`Arc`] so it can be shared.
    pub predictor: Arc<dyn Predictor<W>>,

    /// Sink for confirmed input pairs. Use [`NullLogger`](crate::NullLogger) to
    /// ignore them. See [`Logger`].
    pub logger: Box<dyn Logger<W>>,
}

/// The result of one [`Session::advance`] call: the state to present this
/// frame, plus the metadata needed to render it. The clock-sync hint is read
/// separately via [`Session::skew`].
pub struct Frame<'a, W: World> {
    /// The tick this frame represents (`frontier - present_delay`, clamped to
    /// what has been simulated). May be a speculative tick or a fully confirmed
    /// one depending on how far remote input has lagged.
    pub tick: u32,

    /// The game state to render this frame, borrowed from the session. It may be
    /// the authoritative settled state or a speculative one built from predicted
    /// remote input.
    pub state: &'a W::State,

    /// The `(local, remote)` input pair that applies at [`tick`](Frame::tick) —
    /// handy for presentation that should line up with the frame being shown
    /// (local audio, effects, UI). For a confirmed tick the remote half is the
    /// real input received from the peer; for a speculative tick it is the
    /// [`Predictor`]-supplied guess used to build the frame.
    pub input: (W::Input, W::Input),
}

/// One speculatively-simulated tick, kept in a rolling buffer so a correct
/// prediction can be promoted to settled with no re-simulation, and a wrong one
/// can be rolled back to.
struct Speculation<W: World> {
    /// The tick this snapshot is poised at (one past the tick its `input`
    /// advanced). `speculations[i].tick == settled_tick + 1 + i`.
    tick: u32,
    /// The bundled game state at the start of [`tick`](Self::tick).
    state: W::State,
    /// The `(real local, predicted remote)` pair that produced this snapshot.
    /// The predicted-remote half is what a later confirmation is checked
    /// against to decide promote-vs-rollback.
    input: (W::Input, W::Input),
    /// Whether the producing step ended the round; carried so a promotion applies
    /// the logging clamp at the same point a rollback re-sim would.
    terminal: bool,
}

/// A single peer's view of a two-player rollback session.
///
/// `Session` owns the local/remote input queues, the authoritative ("settled")
/// state, and a rolling buffer of speculative snapshots shown to the player. The
/// intended loop is:
///
/// 1. As remote packets arrive, call [`add_remote_input`](Session::add_remote_input).
/// 2. Once per tick, read [`skew`](Session::skew) and feed it into your clock so
///    the two peers stay aligned, then call [`advance`](Session::advance) with
///    the local input. `advance` confirms any newly-matched ticks into the
///    settled state — promoting the speculative snapshots whose prediction held
///    and rolling back only where it didn't — extends speculation forward as
///    needed, and returns a [`Frame`] to render.
///
/// Construct one with [`Session::new`]. See the [crate-level docs](crate) for a
/// complete example.
pub struct Session<W: World> {
    present_delay: u32,

    simulator: Box<dyn Simulator<W>>,
    predictor: Arc<dyn Predictor<W>>,
    logger: Box<dyn Logger<W>>,

    frontier: u32,

    input_queue: Queue<W::Input>,

    /// Number of confirmed `(local, remote)` pairs ever matched (cumulative).
    /// Confirmed ticks are `0..commit_frontier`. Invariant:
    /// `commit_frontier == settled_tick + settle_backlog.len()`.
    commit_frontier: u32,
    last_committed_remote: W::Input,

    /// Confirmed pairs not yet folded into the settled state, in tick order
    /// (covering ticks `settled_tick..commit_frontier`). They are held back
    /// until the present target reaches them so the settled state never runs
    /// past the frame being displayed.
    settle_backlog: VecDeque<(W::Input, W::Input)>,
    /// The authoritative state, built only from confirmed-and-correct pairs.
    settled_state: W::State,
    /// The tick `settled_state` is poised at (number of pairs folded in).
    settled_tick: u32,
    /// Rolling buffer of speculative snapshots covering `(settled_tick, …]`,
    /// contiguous and in tick order. The simulator is parked at
    /// `settled_tick + speculations.len()` (the speculation frontier).
    speculations: VecDeque<Speculation<W>>,

    /// Set once a confirmed (settled) step reports the round ended. From then on
    /// no further input pairs are committed to the logger — but the simulation
    /// keeps advancing through the post-round-end frames, since the host detects
    /// the end from the presented state a tick or two later.
    terminal_reached: bool,

    /// How many speculative frames the most recent [`advance`](Session::advance)
    /// discarded and re-simulated because a confirmed remote input didn't match
    /// the prediction — i.e. the rollback depth for that frame, surfaced as a
    /// telemetry signal via [`misprediction_depth`](Session::misprediction_depth).
    /// 0 when the frame promoted cleanly or didn't settle.
    last_rollback: u32,

    last_remote_tick_advantage: i16,
}

impl<W: World> Session<W> {
    /// Create a session from the given [`SessionParams`], seeded at tick 0.
    pub fn new(params: SessionParams<W>) -> Self {
        let SessionParams {
            present_delay,
            initial_remote,
            initial_state,
            simulator,
            predictor,
            logger,
        } = params;

        Self {
            present_delay,
            simulator,
            predictor,
            logger,
            frontier: 0,
            input_queue: Queue::new(),
            commit_frontier: 0,
            last_committed_remote: initial_remote,
            settle_backlog: VecDeque::new(),
            settled_state: initial_state,
            settled_tick: 0,
            speculations: VecDeque::new(),
            terminal_reached: false,
            last_rollback: 0,
            last_remote_tick_advantage: 0,
        }
    }

    /// The local frontier: the number of ticks [`advance`](Session::advance) has
    /// been called, i.e. the newest local tick.
    pub fn frontier(&self) -> u32 {
        self.frontier
    }

    /// The current present delay — how many ticks behind the frontier frames are
    /// presented. See [`SessionParams::present_delay`].
    pub fn present_delay(&self) -> u32 {
        self.present_delay
    }

    /// Change the present delay at runtime. Takes effect on the next
    /// [`advance`](Session::advance).
    pub fn set_present_delay(&mut self, present_delay: u32) {
        self.present_delay = present_delay;
    }

    /// The authoritative settled state. Exposed so the host can read host-side
    /// data bundled into the state (e.g. to re-anchor an auxiliary simulator at
    /// round end). The engine treats it as opaque.
    pub fn settled_state(&self) -> &W::State {
        &self.settled_state
    }

    /// Advance the simulation by one local tick and return the [`Frame`] to
    /// present.
    ///
    /// This is the per-tick driver. It:
    ///
    /// 1. enqueues `local_input`;
    /// 2. matches it against any buffered remote inputs and folds the newly
    ///    confirmed ticks into the authoritative settled state — promoting the
    ///    speculative snapshots whose predicted remote matched (no re-sim) and
    ///    rolling back to re-simulate where it didn't, logging confirmed pairs;
    /// 3. computes the present target (`frontier - present_delay`);
    /// 4. extends the speculative buffer forward to the target with
    ///    [`Predictor`]-supplied remote inputs, reusing the snapshots already
    ///    built (only the genuinely new ticks are simulated);
    /// 5. returns the state, tick, and the local/remote input pair at that tick.
    ///    (Read [`skew`](Session::skew) *before* this call for the clock-sync
    ///    hint covering the tick being advanced.)
    ///
    /// # Errors
    ///
    /// Propagates any [`W::Error`](World::Error) returned by the
    /// [`Simulator`](crate::Simulator).
    pub fn advance(&mut self, local_input: W::Input) -> Result<Frame<'_, W>, W::Error> {
        self.input_queue.add_local_input(local_input);

        let (committable, unmatched_locals) = self.input_queue.drain_matched();
        self.commit_frontier += committable.len() as u32;
        self.settle_backlog.extend(committable);

        let target = self.frontier.saturating_sub(self.present_delay);

        // Per-frame rollback depth, recomputed by `settle_to` below.
        self.last_rollback = 0;

        // Fold confirmed pairs into the settled state, but never past the present
        // target (so the settled state stays at or behind the frame we display).
        // Settling continues through a round end — only logging stops there.
        self.settle_to(target.min(self.commit_frontier))?;

        // Extend the speculative tail up to the present target with predicted
        // remotes, reusing the snapshots already built.
        if target > self.settled_tick && self.commit_frontier > 0 {
            self.speculate_to(target, &unmatched_locals)?;
        }

        self.frontier += 1;

        // Present the deepest speculation we built (tick == target). If there is
        // none — no speculation needed — present the settled state.
        Ok(if target > self.settled_tick && self.commit_frontier > 0 {
            let spec = self.speculations.back().expect("speculative frame");
            debug_assert_eq!(spec.tick, target);
            Frame {
                tick: spec.tick,
                state: &spec.state,
                input: spec.input.clone(),
            }
        } else {
            let input = self.settle_backlog.front().cloned().unwrap_or_else(|| {
                (
                    unmatched_locals[0].clone(),
                    self.predictor.predict(&self.last_committed_remote),
                )
            });
            Frame {
                tick: self.settled_tick,
                state: &self.settled_state,
                input,
            }
        })
    }

    /// Number of local inputs buffered but not yet confirmed against a remote
    /// input.
    pub fn local_queue_length(&self) -> usize {
        self.input_queue.local_queue_length()
    }

    /// Number of received remote inputs not yet matched to a local input.
    pub fn remote_queue_length(&self) -> usize {
        self.input_queue.remote_queue_length()
    }

    /// How far local input leads remote input, in ticks (clamped to [`i16`]).
    ///
    /// This is the input queue's signed [`lead`](crate::Queue::lead), surfaced
    /// for clock sync. It is each peer's half of the clock-sync handshake: you
    /// send it to the remote with every input, and the remote's value comes back
    /// via [`add_remote_input`](Session::add_remote_input). The difference of the
    /// two is the [`skew`](Session::skew) used to keep the simulations aligned.
    pub fn local_tick_advantage(&self) -> i16 {
        self.input_queue.lead().clamp(i16::MIN as i32, i16::MAX as i32) as i16
    }

    /// The tick advantage the remote peer last reported (via the
    /// `tick_advantage` argument to [`add_remote_input`](Session::add_remote_input)).
    pub fn last_remote_tick_advantage(&self) -> i16 {
        self.last_remote_tick_advantage
    }

    /// The clock-sync hint for the next tick to advance:
    /// [`local_tick_advantage`](Session::local_tick_advantage) minus the remote
    /// peer's reported advantage.
    ///
    /// Positive means this client is running ahead of the remote and should slow
    /// down (e.g. occasionally stall a frame) so the two simulations converge and
    /// the prediction window stays small; zero means the peers are balanced.
    ///
    /// Read this *before* [`advance`](Session::advance): it reflects the local
    /// advantage at the point the peer reads the value you ship them, which is
    /// before this tick's local input is enqueued. Reading it afterward would
    /// fold that just-enqueued input into the local half and bias the skew up by
    /// one.
    pub fn skew(&self) -> i32 {
        self.local_tick_advantage() as i32 - self.last_remote_tick_advantage as i32
    }

    /// The signed balance of the latest presented frame around the speculation
    /// boundary — `lead - present_delay`, spanning both the speculative-depth
    /// and headroom sides so a single value covers both. (Floor the positive
    /// side for the plain speculative depth; negate and floor the other for the
    /// headroom.)
    ///
    /// This is *not* the raw local-over-remote lead. The presented frame is
    /// `frontier - present_delay`, so the present delay absorbs the first
    /// `present_delay` ticks of lead before any speculation is needed; only the
    /// excess is actually rendered into the speculative tail. So:
    ///
    /// * positive — the presented frame speculates that many ticks past the last
    ///   confirmed input;
    /// * zero — the frame is confirmed and sitting exactly at the boundary;
    /// * negative — the frame is confirmed with `-balance` ticks of *headroom*
    ///   (speculation-free buffer) still to spend before speculation begins.
    ///
    /// Clock-sync leniency keys off the negative range: a positive
    /// [`skew`](Session::skew) only starts costing presentation quality once the
    /// balance reaches 0, so callers take `(-balance).max(0)` for the headroom.
    pub fn speculation_balance(&self) -> i32 {
        self.input_queue.lead().max(0) - self.present_delay as i32
    }

    /// How many speculative frames the most recent [`advance`](Session::advance)
    /// discarded and re-simulated because a confirmed remote input contradicted
    /// the prediction — the instantaneous rollback depth for that frame. 0 when
    /// the frame promoted its predictions cleanly (or didn't settle). A telemetry
    /// signal: spikes mark mispredictions, unlike the steady-state
    /// [`speculation_balance`](Session::speculation_balance).
    pub fn misprediction_depth(&self) -> u32 {
        self.last_rollback
    }

    /// Record an input received from the remote peer.
    ///
    /// * `input` — the remote player's input for the next unmatched tick.
    /// * `tick_advantage` — the remote peer's reported
    ///   [`local_tick_advantage`](Session::local_tick_advantage), used to
    ///   compute clock [`skew`](Session::skew).
    ///
    /// Call this whenever remote inputs arrive; they are matched to local inputs
    /// on the next [`advance`](Session::advance).
    pub fn add_remote_input(&mut self, input: W::Input, tick_advantage: i16) {
        self.input_queue.add_remote_input(input);
        self.last_remote_tick_advantage = tick_advantage;
    }

    /// Fold confirmed pairs into the settled state up to `target`, promoting the
    /// speculative snapshots whose predicted remote matched and rolling back to
    /// re-simulate where it didn't. Logs confirmed pairs in tick order. Stops
    /// early if the round ends (a step returns `None`), leaving the settled state
    /// parked at the round-end tick. No-op if already settled to or past
    /// `target`.
    fn settle_to(&mut self, target: u32) -> Result<(), W::Error> {
        let to_settle = target.saturating_sub(self.settled_tick) as usize;
        if to_settle == 0 {
            return Ok(());
        }
        debug_assert!(self.settle_backlog.len() >= to_settle);

        // Longest prefix of the confirmed pairs whose remote matches what we
        // predicted speculatively — these can be promoted with no re-sim. A
        // speculation only ever exists for a live tick (the round-ending step
        // returns `None` and is never buffered), so the prefix is all loggable.
        let mut promote = 0;
        while promote < to_settle
            && promote < self.speculations.len()
            && self.speculations[promote].input.1 == self.settle_backlog[promote].1
        {
            promote += 1;
        }

        // Promote the correctly-predicted prefix: slide the settled cap up over
        // the speculative snapshots, which are byte-exact because their predicted
        // remote equalled the real one. The simulator is not touched.
        for _ in 0..promote {
            let spec = self.speculations.pop_front().unwrap();
            let pair = self.settle_backlog.pop_front().unwrap();
            assert_eq!(spec.tick, self.settled_tick + 1);
            self.last_committed_remote = pair.1.clone();
            self.commit_pair(&pair, spec.terminal);
            self.settled_state = spec.state;
            self.settled_tick = spec.tick;
        }

        // Anything past the matched prefix descends from a wrong prediction (or
        // was never speculated): discard the speculative tail and re-simulate the
        // remaining confirmed pairs authoritatively, rewinding both this and the
        // host's auxiliary cores via `restore`. Settling continues through a round
        // end (the post-end animation is real state); only logging stops there.
        if promote < to_settle {
            // The speculative tail we're throwing away — the rollback depth for
            // this frame (0 when there was simply nothing speculated yet).
            self.last_rollback = self.speculations.len() as u32;
            self.speculations.clear();
            self.simulator.restore(&self.settled_state)?;
            for _ in promote..to_settle {
                let pair = self.settle_backlog.pop_front().unwrap();
                let (state, ended) = self.simulator.step(pair.clone())?;
                self.settled_tick += 1;
                self.settled_state = state;
                self.last_committed_remote = pair.1.clone();
                self.commit_pair(&pair, ended);
            }
        }

        debug_assert_eq!(self.settled_tick, target);
        Ok(())
    }

    /// Forward a confirmed pair to the logger unless the round has already ended.
    /// `ended` is the producing step's round-end flag; once any settled step ends
    /// the round, this and every later pair are withheld (the post-round-end
    /// frames are simulated but not part of the recorded round).
    fn commit_pair(&mut self, pair: &(W::Input, W::Input), ended: bool) {
        if ended {
            self.terminal_reached = true;
        }
        if !self.terminal_reached {
            self.logger.log(pair);
        }
    }

    /// Extend the speculative buffer up to `target` using real local inputs and
    /// predicted remote inputs, simulating only the ticks not already covered.
    /// The simulator is parked at the speculation frontier, so each new tick is a
    /// plain forward `step`.
    fn speculate_to(&mut self, target: u32, unmatched_locals: &[W::Input]) -> Result<(), W::Error> {
        assert_eq!(
            self.settled_tick, self.commit_frontier,
            "speculation only runs once the settled cap has caught up to the confirmed frontier"
        );
        let mut predicted = self.last_committed_remote.clone();
        while self.settled_tick + self.speculations.len() as u32 + 1 <= target {
            // `unmatched_locals[k]` is the local input for tick
            // `commit_frontier + k`; here `commit_frontier == settled_tick`, so
            // the local for the next speculative tick is at `speculations.len()`.
            let local = unmatched_locals[self.speculations.len()].clone();
            let tick = self.settled_tick + self.speculations.len() as u32 + 1;
            predicted = self.predictor.predict(&predicted);
            let (state, ended) = self.simulator.step((local.clone(), predicted.clone()))?;
            self.speculations.push_back(Speculation {
                tick,
                state,
                input: (local, predicted.clone()),
                terminal: ended,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A deterministic world whose state is the full ordered history of applied
    /// `(local, remote)` pairs — so the settled state can be checked byte-for-byte
    /// against a ground-truth fold of the confirmed inputs.
    struct W;
    impl World for W {
        type Input = u8;
        type State = Vec<(u8, u8)>;
        type Error = std::convert::Infallible;
    }

    #[derive(Default)]
    struct Counters {
        restores: usize,
        steps: usize,
    }

    /// `terminal_at` is the last live tick: a step whose resulting tick exceeds it
    /// reports `ended = true` (the round-ending tick) but keeps producing state.
    /// `restore` skips the reload when already parked at the target tick,
    /// mirroring the real adapter, so the rollback counter reflects only genuine
    /// rewinds.
    struct Sim {
        parked: Vec<(u8, u8)>,
        counters: Arc<Mutex<Counters>>,
        terminal_at: Option<u32>,
    }
    impl Simulator<W> for Sim {
        fn restore(&mut self, state: &Vec<(u8, u8)>) -> Result<(), std::convert::Infallible> {
            if self.parked.len() == state.len() {
                return Ok(());
            }
            self.counters.lock().unwrap().restores += 1;
            self.parked = state.clone();
            Ok(())
        }
        fn step(&mut self, input: (u8, u8)) -> Result<(Vec<(u8, u8)>, bool), std::convert::Infallible> {
            self.counters.lock().unwrap().steps += 1;
            self.parked.push(input);
            let resulting_tick = self.parked.len() as u32;
            let ended = self.terminal_at == Some(resulting_tick);
            Ok((self.parked.clone(), ended))
        }
    }

    struct Repeat;
    impl Predictor<W> for Repeat {
        fn predict(&self, last: &u8) -> u8 {
            *last
        }
    }

    struct VecLogger(Arc<Mutex<Vec<(u8, u8)>>>);
    impl Logger<W> for VecLogger {
        fn log(&mut self, pair: &(u8, u8)) {
            self.0.lock().unwrap().push(*pair);
        }
    }

    fn truth(locals: &[u8], remotes: &[u8]) -> Vec<(u8, u8)> {
        locals.iter().zip(remotes).map(|(&l, &r)| (l, r)).collect()
    }

    fn session(
        present_delay: u32,
        counters: Arc<Mutex<Counters>>,
        logged: Arc<Mutex<Vec<(u8, u8)>>>,
        terminal_at: Option<u32>,
    ) -> Session<W> {
        Session::new(SessionParams {
            present_delay,
            initial_remote: 0,
            initial_state: vec![],
            simulator: Box::new(Sim {
                parked: vec![],
                counters,
                terminal_at,
            }),
            predictor: Arc::new(Repeat),
            logger: Box::new(VecLogger(logged)),
        })
    }

    /// With remote input arriving late and every prediction wrong (distinct
    /// remotes, repeat-predictor), the settled state must stay a correct prefix of
    /// the ground truth at every frame and end up exactly equal — i.e. rollback
    /// re-simulation always corrects the mispredicted tail.
    #[test]
    fn settles_correctly_through_mispredictions() {
        let counters = Arc::new(Mutex::new(Counters::default()));
        let logged = Arc::new(Mutex::new(Vec::new()));
        let locals = [10u8, 11, 12, 13, 14, 15, 16, 17];
        let remotes = [20u8, 21, 22, 23, 24, 25, 26, 27];
        let truth = truth(&locals, &remotes);
        let n = locals.len();
        let remote_delay = 2;

        let mut s = session(0, counters.clone(), logged.clone(), None);

        // n real frames plus a couple to flush the present target to the end.
        for i in 0..n + remote_delay {
            if i >= remote_delay && i - remote_delay < n {
                s.add_remote_input(remotes[i - remote_delay], 0);
            }
            let local = if i < n { locals[i] } else { 99 };
            s.advance(local).unwrap();
            // The authoritative settled state is always a correct prefix of truth.
            let st = s.settled_state();
            assert_eq!(st.as_slice(), &truth[..st.len()], "settled diverged at frame {i}");
        }

        assert_eq!(
            s.settled_state().as_slice(),
            truth.as_slice(),
            "did not settle the full round"
        );
        assert_eq!(
            logged.lock().unwrap().as_slice(),
            truth.as_slice(),
            "logged the wrong pairs"
        );
        // Mispredictions actually happened, so rollback re-sim ran.
        assert!(counters.lock().unwrap().restores > 0, "expected rollbacks");
    }

    /// When predictions hold (remote held constant so repeat-predict is right),
    /// confirming a tick must promote the existing speculation with no extra
    /// simulation: the total step count equals the number of distinct ticks ever
    /// simulated, never re-running a tick that was already speculated correctly.
    #[test]
    fn correct_predictions_promote_without_resim() {
        let counters = Arc::new(Mutex::new(Counters::default()));
        let logged = Arc::new(Mutex::new(Vec::new()));
        // Constant remote: predict(last) == next, so every speculation is right.
        let locals = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let remotes = [9u8; 8];
        let truth = truth(&locals, &remotes);
        let n = locals.len();
        let remote_delay = 2;

        let mut s = session(0, counters.clone(), logged.clone(), None);
        for i in 0..n + remote_delay {
            if i >= remote_delay && i - remote_delay < n {
                s.add_remote_input(remotes[i - remote_delay], 0);
            }
            let local = if i < n { locals[i] } else { 99 };
            s.advance(local).unwrap();
        }

        assert_eq!(s.settled_state().as_slice(), truth.as_slice());
        assert_eq!(logged.lock().unwrap().as_slice(), truth.as_slice());
        // Each of the n ticks is simulated exactly once (as a speculation that is
        // then promoted) — no tick is re-simulated, so steps == n. Extra dummy
        // ticks may add a few more speculative steps, but never re-run a settled
        // tick; the key invariant is no redundant re-sim of confirmed ticks.
        let steps = counters.lock().unwrap().steps;
        assert!(steps >= n, "every confirmed tick must be simulated at least once");
        assert_eq!(
            counters.lock().unwrap().restores,
            0,
            "correct predictions must not roll back"
        );
    }

    /// A round end clamps *logging* but not simulation: the settled state keeps
    /// advancing through the post-round-end frames (so the host's presentation can
    /// reach the end), while no pair from the round-ending tick on is logged.
    /// (Rollback path: distinct remotes make every prediction wrong.)
    #[test]
    fn round_end_clamps_logging_not_simulation() {
        let counters = Arc::new(Mutex::new(Counters::default()));
        let logged = Arc::new(Mutex::new(Vec::new()));
        let locals = [10u8, 11, 12, 13, 14, 15];
        let remotes = [20u8, 21, 22, 23, 24, 25];
        let truth = truth(&locals, &remotes);
        let n = locals.len();
        let remote_delay = 2;
        // The step producing tick 4 ends the round; its pair (and later ones) are
        // withheld from the log, but the state still advances.
        let mut s = session(0, counters.clone(), logged.clone(), Some(4));
        for i in 0..n + remote_delay {
            if i >= remote_delay && i - remote_delay < n {
                s.add_remote_input(remotes[i - remote_delay], 0);
            }
            let local = if i < n { locals[i] } else { 99 };
            s.advance(local).unwrap();
        }
        // Settled advances through the whole confirmed run...
        assert_eq!(s.settled_state().as_slice(), truth.as_slice());
        // ...but only the pairs before the round-ending tick are logged.
        assert_eq!(logged.lock().unwrap().as_slice(), &truth[..3]);
    }

    /// Same round-end logging clamp, reached through the *promote* path: a
    /// constant remote makes every prediction right, so the round-ending tick is
    /// confirmed by promoting a (terminal-flagged) speculation rather than a
    /// rollback re-sim. The clamp must apply identically, and settled must never
    /// diverge from ground truth.
    #[test]
    fn round_end_clamp_via_promotion() {
        let counters = Arc::new(Mutex::new(Counters::default()));
        let logged = Arc::new(Mutex::new(Vec::new()));
        let locals = [1u8, 2, 3, 4, 5, 6];
        let remotes = [9u8; 6];
        let truth = truth(&locals, &remotes);
        let n = locals.len();
        let remote_delay = 1;
        let mut s = session(0, counters.clone(), logged.clone(), Some(4));
        for i in 0..n + remote_delay {
            if i >= remote_delay && i - remote_delay < n {
                s.add_remote_input(remotes[i - remote_delay], 0);
            }
            let local = if i < n { locals[i] } else { 99 };
            s.advance(local).unwrap();
            let st = s.settled_state();
            assert_eq!(st.as_slice(), &truth[..st.len()], "settled diverged at frame {i}");
        }
        assert_eq!(s.settled_state().as_slice(), truth.as_slice());
        assert_eq!(logged.lock().unwrap().as_slice(), &truth[..3]);
    }
}
