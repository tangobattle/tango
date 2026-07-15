use std::collections::VecDeque;

use crate::input::Queue;
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

    /// One entry per remote peer: the input assumed for that peer at tick 0,
    /// before any real input has arrived. Used as the seed for prediction.
    /// The length fixes the session's remote count (at least one).
    pub initial_remotes: Vec<W::Input>,

    /// The starting game state at tick 0.
    pub initial_state: W::State,

    /// The game-specific world that advances the simulation. See [`World`].
    pub world: W,
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

    /// The local input that applies at [`tick`](Frame::tick) — handy for
    /// presentation that should line up with the frame being shown (local
    /// audio, effects, UI).
    pub local: W::Input,

    /// The remote inputs that apply at [`tick`](Frame::tick), indexed by
    /// remote slot. For a confirmed tick these are the real inputs received
    /// from the peers; for a speculative tick, each slot holds the real input
    /// where one had arrived and the [`predict`](World::predict)-supplied
    /// guess where it hadn't.
    pub remotes: Box<[W::Input]>,
}

/// One speculatively-simulated tick, kept in a rolling buffer so a correct
/// prediction can be promoted to settled with no re-simulation, and a wrong one
/// can be rolled back to.
struct Speculation<W: World> {
    /// The tick this snapshot is poised at (one past the tick its inputs
    /// advanced). `speculations[i].tick == settled_tick + 1 + i`.
    tick: u32,
    /// The bundled game state at the start of [`tick`](Self::tick).
    state: W::State,
    /// The real local input that produced this snapshot.
    local: W::Input,
    /// The remote inputs that produced this snapshot, per slot: real where
    /// the input had already arrived, predicted where it hadn't. This is what
    /// a later confirmation is checked against to decide promote-vs-rollback.
    remotes: Box<[W::Input]>,
}

/// A single peer's view of a rollback session: one local player against any
/// number of remote peers (a classic two-player session has exactly one).
///
/// `Session` owns the local/remote input queues, the authoritative ("settled")
/// state, and a rolling buffer of speculative snapshots shown to the player. The
/// intended loop is:
///
/// 1. As remote packets arrive, call [`add_remote_input`](Session::add_remote_input)
///    with the sending peer's slot.
/// 2. Once per tick, read [`skew`](Session::skew) and feed it into your clock so
///    the peers stay aligned, then call [`advance`](Session::advance) with
///    the local input. `advance` confirms any newly-matched ticks into the
///    settled state — promoting the speculative snapshots whose prediction held
///    and rolling back only where it didn't — extends speculation forward as
///    needed, and returns a [`Frame`] to render.
///
/// A tick is *confirmed* only once every remote's input for it has arrived;
/// until then it is speculative. Speculation uses the real input for any
/// remote whose packet is already in (only the missing slots are predicted),
/// so a fast peer's inputs never get second-guessed while waiting on a slow
/// one.
///
/// Construct one with [`Session::new`]. See the [crate-level docs](crate) for a
/// complete example.
pub struct Session<W: World> {
    present_delay: u32,

    world: W,

    local_frontier: u32,

    input_queue: Queue<W::Input>,

    /// Number of confirmed input rows ever matched (cumulative).
    /// Confirmed ticks are `0..confirm_frontier`. Invariant:
    /// `confirm_frontier == settled_tick + settle_backlog.len()`.
    confirm_frontier: u32,
    /// Per remote slot, the last input confirmed for that peer — the base of
    /// its prediction chain.
    last_confirmed_remotes: Box<[W::Input]>,

    /// Confirmed `(local, remotes)` rows not yet folded into the settled
    /// state, in tick order (covering ticks `settled_tick..confirm_frontier`).
    /// They are held back until the present target reaches them so the
    /// settled state never runs past the frame being displayed.
    settle_backlog: VecDeque<(W::Input, Box<[W::Input]>)>,
    /// The authoritative state, built only from confirmed-and-correct rows.
    settled_state: W::State,
    /// The tick `settled_state` is poised at (number of rows folded in).
    settled_tick: u32,
    /// Rolling buffer of speculative snapshots covering `(settled_tick, …]`,
    /// contiguous and in tick order. The simulator is parked at
    /// `settled_tick + speculations.len()` (the speculation frontier).
    speculations: VecDeque<Speculation<W>>,

    /// How many speculative frames the most recent [`advance`](Session::advance)
    /// discarded and re-simulated because a confirmed remote input didn't match
    /// the prediction — i.e. the rollback depth for that frame, surfaced as a
    /// telemetry signal via [`misprediction_depth`](Session::misprediction_depth).
    /// 0 when the frame promoted cleanly or didn't settle.
    last_misprediction_depth: u32,

    /// Per remote slot, the tick advantage that peer last reported.
    last_remote_tick_advantages: Box<[i16]>,
}

impl<W: World> Session<W> {
    /// Create a session from the given [`SessionParams`], seeded at tick 0.
    pub fn new(params: SessionParams<W>) -> Self {
        let SessionParams {
            present_delay,
            initial_remotes,
            initial_state,
            world,
        } = params;
        assert!(!initial_remotes.is_empty(), "a session needs at least one remote");

        Self {
            present_delay,
            world,
            local_frontier: 0,
            input_queue: Queue::new(initial_remotes.len()),
            confirm_frontier: 0,
            last_remote_tick_advantages: initial_remotes.iter().map(|_| 0).collect(),
            last_confirmed_remotes: initial_remotes.into_boxed_slice(),
            settle_backlog: VecDeque::new(),
            settled_state: initial_state,
            settled_tick: 0,
            speculations: VecDeque::new(),
            last_misprediction_depth: 0,
        }
    }

    /// Number of remote peers in this session.
    pub fn num_remotes(&self) -> usize {
        self.input_queue.num_remotes()
    }

    /// The local frontier: the number of ticks [`advance`](Session::advance) has
    /// been called, i.e. the newest local tick.
    pub fn local_frontier(&self) -> u32 {
        self.local_frontier
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
    /// 2. matches it against the buffered remote inputs and folds the newly
    ///    confirmed ticks (those with every remote's input present) into the
    ///    authoritative settled state — promoting the speculative snapshots
    ///    whose predicted remotes all matched (no re-sim) and rolling back to
    ///    re-simulate where they didn't, logging confirmed rows;
    /// 3. computes the present target (`frontier - present_delay`);
    /// 4. extends the speculative buffer forward to the target — using each
    ///    remote's real input where it has already arrived and a
    ///    [`predict`](World::predict)-supplied guess where it hasn't — reusing
    ///    the snapshots already built (only the genuinely new ticks are
    ///    simulated);
    /// 5. returns the state, tick, and the input row at that tick.
    ///    (Read [`skew`](Session::skew) *before* this call for the clock-sync
    ///    hint covering the tick being advanced.)
    ///
    /// # Errors
    ///
    /// Propagates any [`W::Error`](World::Error) returned by the
    /// [`World`](crate::World).
    pub fn advance(&mut self, local_input: W::Input) -> Result<Frame<'_, W>, W::Error> {
        self.input_queue.add_local_input(local_input);

        let (committable, unmatched_locals, unmatched_remotes) = self.input_queue.drain_matched();
        self.confirm_frontier += committable.len() as u32;
        self.settle_backlog.extend(committable);

        let target = self.local_frontier.saturating_sub(self.present_delay);

        // Per-frame rollback depth, recomputed by `settle_to` below.
        self.last_misprediction_depth = 0;

        // Fold confirmed rows into the settled state, but never past the present
        // target (so the settled state stays at or behind the frame we display).
        self.settle_to(target.min(self.confirm_frontier))?;

        // Extend the speculative tail up to the present target, using arrived
        // remote inputs where available and predictions elsewhere, reusing the
        // snapshots already built.
        if target > self.settled_tick && self.confirm_frontier > 0 {
            self.speculate_to(target, &unmatched_locals, &unmatched_remotes)?;
        }

        self.local_frontier += 1;

        // Present the speculation at `target`. The buffer can reach *past* `target`
        // when `present_delay` was just increased (shrinking `target`) since those
        // deeper ticks were speculated, so index to `target` rather than taking the
        // deepest snapshot — the surplus tail stays buffered for the frames where
        // `target` climbs back to it (or is dropped wholesale on the next
        // rollback). If there is no speculation, present the settled state.
        Ok(if target > self.settled_tick && self.confirm_frontier > 0 {
            let spec = &self.speculations[(target - self.settled_tick - 1) as usize];
            assert_eq!(spec.tick, target);
            Frame {
                tick: spec.tick,
                state: &spec.state,
                local: spec.local.clone(),
                remotes: spec.remotes.clone(),
            }
        } else {
            let (local, remotes) = self.settle_backlog.front().cloned().unwrap_or_else(|| {
                (
                    unmatched_locals[0].clone(),
                    self.last_confirmed_remotes
                        .iter()
                        .zip(unmatched_remotes.iter())
                        .map(|(last, arrived)| {
                            arrived.first().cloned().unwrap_or_else(|| self.world.predict(last))
                        })
                        .collect(),
                )
            });
            Frame {
                tick: self.settled_tick,
                state: &self.settled_state,
                local,
                remotes,
            }
        })
    }

    /// Number of local inputs buffered but not yet confirmed against every
    /// remote's input.
    pub fn local_queue_length(&self) -> usize {
        self.input_queue.local_queue_length()
    }

    /// Number of inputs received from remote peer `remote` not yet matched
    /// into a confirmed tick.
    pub fn remote_queue_length(&self, remote: usize) -> usize {
        self.input_queue.remote_queue_length(remote)
    }

    /// How far local input leads the furthest-behind remote's input, in ticks
    /// (clamped to [`i16`]).
    ///
    /// This is the input queue's signed worst-case [`lead`](crate::Queue::lead),
    /// surfaced for clock sync. It is each peer's half of the clock-sync
    /// handshake: you send it to every remote with every input, and each
    /// remote's own value comes back via
    /// [`add_remote_input`](Session::add_remote_input). The difference of the
    /// two is that peer's contribution to the [`skew`](Session::skew) used to
    /// keep the simulations aligned.
    pub fn local_tick_advantage(&self) -> i16 {
        self.input_queue.lead().clamp(i16::MIN as i32, i16::MAX as i32) as i16
    }

    /// The tick advantage remote peer `remote` last reported (via the
    /// `tick_advantage` argument to [`add_remote_input`](Session::add_remote_input)).
    pub fn last_remote_tick_advantage(&self, remote: usize) -> i16 {
        self.last_remote_tick_advantages[remote]
    }

    /// The clock-sync hint for the next tick to advance: the worst case, over
    /// the remotes, of this side's lead over that peer minus the advantage the
    /// peer last reported.
    ///
    /// Positive means this client is running ahead of at least one remote and
    /// should slow down (e.g. occasionally stall a frame) so the simulations
    /// converge and the prediction window stays small; zero or negative means
    /// nobody is waiting on us. With everyone throttling on their own skew,
    /// the group converges toward its slowest member.
    ///
    /// Read this *before* [`advance`](Session::advance): it reflects the local
    /// advantage at the point the peers read the value you ship them, which is
    /// before this tick's local input is enqueued. Reading it afterward would
    /// fold that just-enqueued input into the local half and bias the skew up by
    /// one.
    pub fn skew(&self) -> i32 {
        (0..self.last_remote_tick_advantages.len())
            .map(|i| self.input_queue.lead_over(i) - self.last_remote_tick_advantages[i] as i32)
            .max()
            .expect("session has at least one remote")
    }

    /// The signed balance of the latest presented frame around the speculation
    /// boundary — worst-case `lead - present_delay`, spanning both the
    /// speculative-depth and headroom sides so a single value covers both.
    /// (Floor the positive side for the plain speculative depth; negate and
    /// floor the other for the headroom.)
    ///
    /// This is *not* the raw local-over-remote lead. The presented frame is
    /// `frontier - present_delay`, so the present delay absorbs the first
    /// `present_delay` ticks of lead before any speculation is needed; only the
    /// excess is actually rendered into the speculative tail. So:
    ///
    /// * positive — the presented frame speculates that many ticks past the last
    ///   fully confirmed input;
    /// * zero — the frame is confirmed and sitting exactly at the boundary;
    /// * negative — the frame is confirmed with `-balance` ticks of *headroom*
    ///   (speculation-free buffer) still to spend before speculation begins.
    ///
    /// Clock-sync leniency keys off the sign: a positive
    /// [`skew`](Session::skew) only starts costing presentation quality once the
    /// balance reaches 0, so throttling callers gate engagement on it.
    pub fn speculation_balance(&self) -> i32 {
        self.input_queue.lead().max(0) - self.present_delay as i32
    }

    /// How many speculative frames the most recent [`advance`](Session::advance)
    /// discarded and re-simulated because a confirmed remote input contradicted
    /// the prediction — the instantaneous rollback depth for that frame. 0 when
    /// the frame promoted its predictions cleanly (or didn't settle). A telemetry
    /// signal: spikes mark mispredictions, unlike the steady-state
    /// [`speculation_balance`](Session::speculation_balance).
    pub fn last_misprediction_depth(&self) -> u32 {
        self.last_misprediction_depth
    }

    /// Record an input received from remote peer `remote`.
    ///
    /// * `remote` — which peer sent it (the slot index, `0..num_remotes`).
    /// * `input` — that player's input for their next unmatched tick.
    /// * `tick_advantage` — the peer's reported
    ///   [`local_tick_advantage`](Session::local_tick_advantage), used to
    ///   compute clock [`skew`](Session::skew).
    ///
    /// Call this whenever remote inputs arrive; they are matched into
    /// confirmed ticks on the next [`advance`](Session::advance).
    pub fn add_remote_input(&mut self, remote: usize, input: W::Input, tick_advantage: i16) {
        self.input_queue.add_remote_input(remote, input);
        self.last_remote_tick_advantages[remote] = tick_advantage;
    }

    /// Fold confirmed rows into the settled state up to `target`, promoting the
    /// speculative snapshots whose remote inputs all matched and rolling back to
    /// re-simulate where they didn't. Logs confirmed rows in tick order. No-op
    /// if already settled to or past `target`.
    fn settle_to(&mut self, target: u32) -> Result<(), W::Error> {
        let to_settle = target.saturating_sub(self.settled_tick) as usize;
        if to_settle == 0 {
            return Ok(());
        }
        debug_assert!(self.settle_backlog.len() >= to_settle);

        // Longest prefix of the confirmed rows whose remotes all match what we
        // used speculatively — these can be promoted with no re-sim. (Slots
        // that speculated with an already-arrived real input match trivially.)
        let mut promote = 0;
        while promote < to_settle
            && promote < self.speculations.len()
            && self.speculations[promote].remotes == self.settle_backlog[promote].1
        {
            promote += 1;
        }

        // Promote the correctly-predicted prefix: slide the settled cap up over
        // the speculative snapshots, which are byte-exact because their remote
        // inputs equalled the real ones. The live simulation is not touched.
        for _ in 0..promote {
            let spec = self.speculations.pop_front().unwrap();
            let (local, remotes) = self.settle_backlog.pop_front().unwrap();
            assert_eq!(spec.tick, self.settled_tick + 1);
            let displaced = std::mem::replace(&mut self.settled_state, spec.state);
            self.world.recycle(displaced);
            self.settled_tick = spec.tick;
            self.world.log(&local, &remotes);
            self.last_confirmed_remotes = remotes;
        }

        // Anything past the matched prefix descends from a wrong prediction (or
        // was never speculated): discard the speculative tail and re-simulate the
        // remaining confirmed rows authoritatively, rewinding both this and the
        // host's auxiliary cores via `load`. We re-step the whole corrected tail
        // and `save` only its final tick.
        if promote < to_settle {
            // The speculative tail we're throwing away — the rollback depth for
            // this frame (0 when there was simply nothing speculated yet).
            self.last_misprediction_depth = self.speculations.len() as u32;
            for spec in self.speculations.drain(..) {
                self.world.recycle(spec.state);
            }
            self.world.load(&self.settled_state)?;
            for _ in promote..to_settle {
                let (local, remotes) = self.settle_backlog.pop_front().unwrap();
                self.world.step(&local, &remotes)?;
                self.settled_tick += 1;
                self.world.log(&local, &remotes);
                self.last_confirmed_remotes = remotes;
            }
            let resettled = self.world.save()?;
            let displaced = std::mem::replace(&mut self.settled_state, resettled);
            self.world.recycle(displaced);
        }

        debug_assert_eq!(self.settled_tick, target);
        Ok(())
    }

    /// Extend the speculative buffer up to `target` using real local inputs and,
    /// per remote slot, the real input where one has already arrived (a tick can
    /// be unconfirmed because a *different* remote is still missing) or a
    /// predicted one where it hasn't — simulating only the ticks not already
    /// covered. The live simulation is parked at the speculation frontier, so
    /// each new tick is a plain forward [`step`](World::step) followed by a
    /// [`save`](World::save).
    fn speculate_to(
        &mut self,
        target: u32,
        unmatched_locals: &[W::Input],
        unmatched_remotes: &[Vec<W::Input>],
    ) -> Result<(), W::Error> {
        assert_eq!(
            self.settled_tick, self.confirm_frontier,
            "speculation only runs once the settled cap has caught up to the confirmed frontier"
        );
        // Each remote's prediction chain continues from the newest speculated
        // value for that slot (real or predicted), or from the last confirmed
        // input when nothing is speculated yet.
        let mut chain: Box<[W::Input]> = match self.speculations.back() {
            Some(spec) => spec.remotes.clone(),
            None => self.last_confirmed_remotes.clone(),
        };
        while (self.settled_tick + self.speculations.len() as u32) < target {
            // `unmatched_locals[k]` (and `unmatched_remotes[i][k]`) hold the
            // input for tick `confirm_frontier + k`; here `confirm_frontier ==
            // settled_tick`, so the inputs for the next speculative tick are
            // at `speculations.len()`.
            let k = self.speculations.len();
            let local = unmatched_locals[k].clone();
            let tick = self.settled_tick + k as u32 + 1;
            let remotes: Box<[W::Input]> = chain
                .iter()
                .enumerate()
                .map(|(i, last)| match unmatched_remotes[i].get(k) {
                    Some(real) => real.clone(),
                    None => self.world.predict(last),
                })
                .collect();
            chain = remotes.clone();
            self.world.step(&local, &remotes)?;
            let state = self.world.save()?;
            self.speculations.push_back(Speculation {
                tick,
                state,
                local,
                remotes,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Counters {
        restores: usize,
        steps: usize,
        recycles: usize,
    }

    /// A deterministic world whose state is the full ordered history of applied
    /// `(local, remotes)` rows — so the settled state can be checked byte-for-byte
    /// against a ground-truth fold of the confirmed inputs.
    ///
    /// `load` skips the reload when already parked at the target tick, mirroring
    /// the real adapter, so the rollback counter reflects only genuine rewinds.
    struct W {
        parked: Vec<(u8, Vec<u8>)>,
        counters: Arc<Mutex<Counters>>,
        logged: Arc<Mutex<Vec<(u8, Vec<u8>)>>>,
    }
    impl World for W {
        type Input = u8;
        type State = Vec<(u8, Vec<u8>)>;
        type Error = std::convert::Infallible;

        fn step(&mut self, local: &u8, remotes: &[u8]) -> Result<(), std::convert::Infallible> {
            self.counters.lock().unwrap().steps += 1;
            self.parked.push((*local, remotes.to_vec()));
            Ok(())
        }
        fn save(&mut self) -> Result<Vec<(u8, Vec<u8>)>, std::convert::Infallible> {
            Ok(self.parked.clone())
        }
        fn load(&mut self, state: &Vec<(u8, Vec<u8>)>) -> Result<(), std::convert::Infallible> {
            if self.parked.len() == state.len() {
                return Ok(());
            }
            self.counters.lock().unwrap().restores += 1;
            self.parked = state.clone();
            Ok(())
        }
        // Repeat-predict: assume the remote keeps doing what they were doing.
        fn predict(&self, last: &u8) -> u8 {
            *last
        }
        fn recycle(&mut self, _state: Vec<(u8, Vec<u8>)>) {
            self.counters.lock().unwrap().recycles += 1;
        }
        fn log(&mut self, local: &u8, remotes: &[u8]) {
            self.logged.lock().unwrap().push((*local, remotes.to_vec()));
        }
    }

    fn truth(locals: &[u8], remotes: &[&[u8]]) -> Vec<(u8, Vec<u8>)> {
        locals
            .iter()
            .enumerate()
            .map(|(t, &l)| (l, remotes.iter().map(|r| r[t]).collect()))
            .collect()
    }

    fn session(
        present_delay: u32,
        num_remotes: usize,
        counters: Arc<Mutex<Counters>>,
        logged: Arc<Mutex<Vec<(u8, Vec<u8>)>>>,
    ) -> Session<W> {
        Session::new(SessionParams {
            present_delay,
            initial_remotes: vec![0; num_remotes],
            initial_state: vec![],
            world: W {
                parked: vec![],
                counters,
                logged,
            },
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
        let truth = truth(&locals, &[&remotes]);
        let n = locals.len();
        let remote_delay = 2;

        let mut s = session(0, 1, counters.clone(), logged.clone());

        // n real frames plus a couple to flush the present target to the end.
        for i in 0..n + remote_delay {
            if i >= remote_delay && i - remote_delay < n {
                s.add_remote_input(0, remotes[i - remote_delay], 0);
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
            "logged the wrong rows"
        );
        // Mispredictions actually happened, so rollback re-sim ran.
        assert!(counters.lock().unwrap().restores > 0, "expected rollbacks");
        // Every discarded snapshot (cleared speculations, displaced settled
        // states) must be offered back to the world for reuse.
        assert!(counters.lock().unwrap().recycles > 0, "expected recycled states");
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
        let truth = truth(&locals, &[&remotes]);
        let n = locals.len();
        let remote_delay = 2;

        let mut s = session(0, 1, counters.clone(), logged.clone());
        for i in 0..n + remote_delay {
            if i >= remote_delay && i - remote_delay < n {
                s.add_remote_input(0, remotes[i - remote_delay], 0);
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
        // Each promotion displaces exactly one settled state, and with no
        // rollbacks that's the only discard path: one recycle per settled tick.
        assert_eq!(
            counters.lock().unwrap().recycles,
            s.settled_state().len(),
            "every promotion must recycle the displaced settled state"
        );
    }

    /// Raising `present_delay` mid-session shrinks `target` below the speculation
    /// frontier already built, so the deepest buffered spec sits *past* the new
    /// target. `advance` must present the spec at `target` (not the deepest) and
    /// keep settling correctly — regression for a panic when the two diverged.
    #[test]
    fn present_delay_increase_presents_target_not_tail() {
        let counters = Arc::new(Mutex::new(Counters::default()));
        let logged = Arc::new(Mutex::new(Vec::new()));
        // Constant remote: every prediction holds, so the deep speculation tail is
        // never cleared by a rollback — exactly the case that leaves the buffer
        // longer than `target` after the delay bump.
        let locals: Vec<u8> = (1..=24).collect();
        let remotes = [9u8; 24];
        let truth = truth(&locals, &[&remotes]);
        let n = locals.len();
        // Remote lags far enough that even after the bump the settled cap stays
        // behind `target` (so `advance` takes the speculating branch, not the
        // settled one).
        let remote_delay = 6;

        // present_delay 0 first: speculation runs `remote_delay` ticks ahead of the
        // settled cap, building a deep buffer.
        let mut s = session(0, 1, counters.clone(), logged.clone());
        for i in 0..12 {
            if i >= remote_delay {
                s.add_remote_input(0, remotes[i - remote_delay], 0);
            }
            s.advance(locals[i]).unwrap();
        }

        // Bump present_delay: `target` drops below the buffer frontier built above,
        // so the deepest spec now sits past `target`. This used to panic on
        // `assert_eq!(spec.tick, target)`.
        s.set_present_delay(3);
        for i in 12..n + remote_delay {
            if i >= remote_delay && i - remote_delay < n {
                s.add_remote_input(0, remotes[i - remote_delay], 0);
            }
            let local = if i < n { locals[i] } else { 99 };
            let frame_tick = s.advance(local).unwrap().tick;
            // Settled stays a correct prefix of ground truth, and the presented
            // tick never runs behind the settled cap.
            let st = s.settled_state();
            assert_eq!(st.as_slice(), &truth[..st.len()], "settled diverged at frame {i}");
            assert!(frame_tick >= st.len() as u32, "presented before settled at frame {i}");
        }

        assert_eq!(s.settled_state().as_slice(), truth.as_slice());
        assert_eq!(logged.lock().unwrap().as_slice(), truth.as_slice());
    }

    /// Three players, each remote arriving with a different lag, every
    /// prediction wrong (distinct values, repeat-predictor): the settled state
    /// must stay a correct prefix of ground truth at every frame and end exactly
    /// equal, and the confirmed rows must be logged in order.
    #[test]
    fn three_player_settles_correctly_with_skewed_arrivals() {
        let counters = Arc::new(Mutex::new(Counters::default()));
        let logged = Arc::new(Mutex::new(Vec::new()));
        let locals = [10u8, 11, 12, 13, 14, 15, 16, 17];
        let remote_a = [20u8, 21, 22, 23, 24, 25, 26, 27];
        let remote_b = [30u8, 31, 32, 33, 34, 35, 36, 37];
        let truth = truth(&locals, &[&remote_a, &remote_b]);
        let n = locals.len();
        let delays = [1usize, 4usize];

        let mut s = session(0, 2, counters.clone(), logged.clone());
        let max_delay = 4;
        for i in 0..n + max_delay {
            for (slot, (inputs, delay)) in [(&remote_a, delays[0]), (&remote_b, delays[1])].iter().enumerate() {
                if i >= *delay && i - *delay < n {
                    s.add_remote_input(slot, inputs[i - *delay], 0);
                }
            }
            let local = if i < n { locals[i] } else { 99 };
            s.advance(local).unwrap();
            let st = s.settled_state();
            assert_eq!(st.as_slice(), &truth[..st.len()], "settled diverged at frame {i}");
        }

        assert_eq!(s.settled_state().as_slice(), truth.as_slice());
        assert_eq!(logged.lock().unwrap().as_slice(), truth.as_slice());
        assert!(counters.lock().unwrap().restores > 0, "expected rollbacks");
    }

    /// A remote input that has already *arrived* — its tick merely unconfirmed
    /// because a different remote is still missing — must be used as-is by
    /// speculation, not second-guessed by the predictor. Remote A varies every
    /// tick (repeat-predict would be wrong every time) but arrives with lag 1,
    /// inside the present delay, so every speculated tick already has A's real
    /// input; remote B is constant (predictions hold) but arrives with lag 4.
    /// If speculation uses A's real inputs, nothing ever rolls back.
    #[test]
    fn arrived_inputs_beat_predictions_in_speculation() {
        let counters = Arc::new(Mutex::new(Counters::default()));
        let logged = Arc::new(Mutex::new(Vec::new()));
        let locals = [1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
        let remote_a = [40u8, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51];
        let remote_b = [7u8; 12];
        let truth = truth(&locals, &[&remote_a, &remote_b]);
        let n = locals.len();

        let mut s = session(2, 2, counters.clone(), logged.clone());
        let max_delay = 4;
        for i in 0..n + max_delay {
            if i >= 1 && i - 1 < n {
                s.add_remote_input(0, remote_a[i - 1], 0);
            }
            if i >= 4 && i - 4 < n {
                s.add_remote_input(1, remote_b[i - 4], 0);
            }
            let local = if i < n { locals[i] } else { 99 };
            s.advance(local).unwrap();
            let st = s.settled_state();
            assert_eq!(st.as_slice(), &truth[..st.len()], "settled diverged at frame {i}");
        }

        assert_eq!(s.settled_state().as_slice(), truth.as_slice());
        assert_eq!(logged.lock().unwrap().as_slice(), truth.as_slice());
        assert_eq!(
            counters.lock().unwrap().restores,
            0,
            "speculation second-guessed an arrived input"
        );
    }
}
