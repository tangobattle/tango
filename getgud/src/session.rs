use std::collections::VecDeque;
use std::sync::Arc;

use crate::input::{Pair, Queue};
use crate::present::Presenter;
use crate::sim::{CommitObserver, Predictor, Simulator};
use crate::throttler::Throttler;
use crate::world::{Snapshot, World};

/// Everything a [`Session`] needs at construction. The host assembles this
/// (capturing its simulator, predictor, replay sink, …) for each new session.
pub struct SessionParams<W: World> {
    /// Display delay in ticks: the displayed state trails the netcode frontier
    /// by this much. Adjustable mid-session via
    /// [`Session::set_frame_delay`].
    pub frame_delay: u32,
    /// Seed for remote prediction before any remote input has committed.
    pub initial_remote: W::Input,
    pub simulator: Box<dyn Simulator<W>>,
    pub predictor: Arc<dyn Predictor<W>>,
    pub observer: Option<Box<dyn CommitObserver<W>>>,
}

/// One session's rollback state machine — the whole engine. A session covers a
/// single bout of play; the host creates a fresh one per round (or match, or
/// whatever its unit of orchestration is) and owns everything around it: the
/// wire, round boundaries, and the lifecycle of any co-simulation.
///
/// Per displayed frame, [`advance`](Session::advance) queues the local input,
/// drains newly-paired inputs onto the commit chain, settles the single
/// checkpoint forward over confirmed inputs, optionally runs a throwaway
/// speculative tail past the commit frontier, presents the chosen state, and
/// emits the time-sync slowdown.
///
/// The host is responsible for sending the local input over the wire (reading
/// [`local_frame_advantage`](Session::local_frame_advantage) for the value to
/// attach) and for feeding received remote inputs in via
/// [`add_remote_input`](Session::add_remote_input).
pub struct Session<W: World> {
    // ---- Constants / config ----
    frame_delay: u32,

    // ---- Seams ----
    simulator: Box<dyn Simulator<W>>,
    predictor: Arc<dyn Predictor<W>>,
    observer: Option<Box<dyn CommitObserver<W>>>,

    // ---- Tick tracking ----
    /// Netcode frontier: advances one per wall-frame, independent of how far
    /// the simulation lags. `frontier - frame_delay` is the present target.
    frontier: u32,
    /// Tick of the state most recently handed to the presenter (0 before any
    /// present).
    presented_tick: u32,

    // ---- Input pipeline ----
    input_queue: Queue<W::Input>,

    // ---- Commit + settled checkpoint ----
    /// Exclusive upper bound of ticks where both peers' real inputs are known.
    commit_frontier: u32,
    /// The most recent remote input the settle committed — seeds the
    /// speculative tail's prediction.
    last_committed_remote: W::Input,
    /// Confirmed input pairs the settle hasn't folded into the checkpoint yet,
    /// covering `[settled.tick, commit_frontier)`. Drained front-first as the
    /// settle advances.
    settle_backlog: VecDeque<Pair<W::Input>>,
    /// The single settled checkpoint that drives display + committed-side
    /// bookkeeping.
    settled_snapshot: Option<Snapshot<W>>,

    // ---- Time sync ----
    /// Count of remote inputs received this session.
    last_remote_received_tick: u32,
    /// The peer's frame advantage as of their most recent input.
    last_remote_frame_advantage: i16,
    throttler: Throttler,
}

impl<W: World> Session<W> {
    pub fn new(params: SessionParams<W>) -> Self {
        let SessionParams {
            frame_delay,
            initial_remote,
            simulator,
            predictor,
            observer,
        } = params;

        Self {
            frame_delay,
            simulator,
            predictor,
            observer,
            frontier: 0,
            presented_tick: 0,
            input_queue: Queue::new(),
            commit_frontier: 0,
            last_committed_remote: initial_remote,
            settle_backlog: std::collections::VecDeque::new(),
            settled_snapshot: None,
            last_remote_received_tick: 0,
            last_remote_frame_advantage: 0,
            throttler: Throttler::new(),
        }
    }

    /// Netcode frontier — advances one per wall-frame regardless of how far the
    /// simulation lags.
    pub fn frontier(&self) -> u32 {
        self.frontier
    }

    /// Tick of the state most recently handed to the presenter (0 before any
    /// present).
    pub fn presented_tick(&self) -> u32 {
        self.presented_tick
    }

    /// How far the displayed state trails the netcode frontier, in ticks.
    pub fn frame_delay(&self) -> u32 {
        self.frame_delay
    }

    /// Adjust the display delay. Takes effect on the next
    /// [`advance`](Session::advance).
    pub fn set_frame_delay(&mut self, frame_delay: u32) {
        self.frame_delay = frame_delay;
    }

    /// Bump the netcode frontier. The host calls this once per wall-frame once
    /// the session is live.
    pub fn advance_frontier(&mut self) {
        self.frontier += 1;
    }

    /// Seed the settled checkpoint at the session's tick-0 state.
    pub fn set_first_settled_state(&mut self, state: W::State) {
        self.settled_snapshot = Some(Snapshot { state, tick: 0 });
    }

    pub fn has_committed_state(&self) -> bool {
        self.settled_snapshot.is_some()
    }

    /// One displayed frame. Queues the local input, settles the checkpoint
    /// forward, speculates past the commit frontier when the display target
    /// demands it, presents the chosen state, and emits the time-sync slowdown.
    ///
    /// The host must already have sent `local_input` over the wire (with
    /// [`local_frame_advantage`](Session::local_frame_advantage) attached)
    /// before calling this — there's no input delay, so what goes on the wire
    /// is exactly what's committed here. The engine has no notion of the bout
    /// ending; the host detects that however it likes (e.g. its own traps).
    pub fn advance(&mut self, presenter: &mut dyn Presenter<W>, local_input: W::Input) -> Result<(), W::Error> {
        self.input_queue.add_local_input(local_input);

        // Drain matched pairs onto the commit chain.
        let (committable, peeked) = self.input_queue.consume_and_peek_local();
        self.commit_frontier += committable.len() as u32;
        self.settle_backlog.extend(committable);

        // Display target: what the user should see this wall-frame.
        let target = self.frontier.saturating_sub(self.frame_delay);

        // Settle forward, capped at the last confirmed tick — the checkpoint
        // must never absorb predicted payloads.
        let settled_target = target.min(self.commit_frontier.saturating_sub(1));
        self.settle_to(settled_target)?;

        if target > settled_target && self.commit_frontier > 0 {
            // Speculative tail: throwaway re-sim from the checkpoint to the
            // target, predicting every remote payload past the frontier.
            let state = self.speculate_tail(target, &peeked)?;
            self.presented_tick = target;
            presenter.present(&state, target);
        } else {
            // Settled (or pre-first-commit): the checkpoint IS the display. Its
            // tick is not always `target` — the wall clock can push `target`
            // past tick 0 before the first remote input lands, and a live
            // frame_delay increase can pull it back behind the cap — so track
            // the tick actually presented.
            let snapshot = self.settled_snapshot.as_ref().unwrap();
            let tick = snapshot.tick;
            presenter.present(&snapshot.state, tick);
            self.presented_tick = tick;
        }

        self.update_slowdown(presenter);
        Ok(())
    }

    /// Local inputs currently queued (committed + speculative). The host reads
    /// this to bound its own feed; the engine imposes no cap.
    pub fn local_queue_length(&self) -> usize {
        self.input_queue.local_queue_length()
    }

    /// Remote inputs currently queued. The host reads this to bound its own
    /// feed; the engine imposes no cap.
    pub fn remote_queue_length(&self) -> usize {
        self.input_queue.remote_queue_length()
    }

    /// "How far ahead of the latest remote input I am." The host attaches this
    /// to each outgoing input so the peer can compute relative real-time skew.
    pub fn local_frame_advantage(&self) -> i16 {
        let diff = self.frontier as i32 - self.last_remote_received_tick as i32;
        diff.clamp(i16::MIN as i32, i16::MAX as i32) as i16
    }

    /// Peer's frame advantage as of their most recent input.
    pub fn last_remote_frame_advantage(&self) -> i16 {
        self.last_remote_frame_advantage
    }

    /// Speculative depth — local inputs queued past the latest remote, i.e. how
    /// many frames a real remote input can force us to re-simulate.
    pub fn speculative_depth(&self) -> u32 {
        self.input_queue.speculative_depth() as u32
    }

    /// Feed a remote input received off the wire, with the peer's frame
    /// advantage. Inputs must arrive in tick order. The host is responsible for
    /// bounding the queue — check
    /// [`remote_queue_length`](Session::remote_queue_length) first.
    pub fn add_remote_input(&mut self, input: W::Input, frame_advantage: i16) {
        self.input_queue.add_remote_input(input);
        self.last_remote_received_tick = self.last_remote_received_tick.wrapping_add(1);
        self.last_remote_frame_advantage = frame_advantage;
    }

    /// Settle the checkpoint forward to `target` (at or behind the commit
    /// frontier). One settle re-sim over `settle_backlog`: the simulator
    /// resolves real remote data internally and the captured state becomes the
    /// next checkpoint. Reports committed pairs to the observer and drops them.
    /// No-op when `target` is at or behind the checkpoint.
    fn settle_to(&mut self, target: u32) -> Result<(), W::Error> {
        let seed_tick = self.settled_snapshot.as_ref().expect("settled state").tick;
        if target <= seed_tick {
            return Ok(());
        }

        let seed = self.settled_snapshot.take().expect("settled state");
        // Inputs for `[seed_tick, target]` — inclusive: the simulator peeks the
        // capture-tick input before snapshotting. `target <= commit_frontier - 1`
        // guarantees `settle_backlog` is long enough.
        let count = (target - seed_tick + 1) as usize;
        debug_assert!(self.settle_backlog.len() >= count);
        let input_pairs: Vec<Pair<W::Input>> = self.settle_backlog.iter().take(count).cloned().collect();

        let result = self.simulator.simulate(&seed, input_pairs, false)?;

        // The last settled pair's remote seeds the speculative tail's
        // prediction. `consumed >= 1` here (we returned early if `target <=
        // seed_tick`), so the index is in range.
        let consumed = (target - seed_tick) as usize;
        self.last_committed_remote = self.settle_backlog[consumed - 1].remote.clone();

        // Report the just-committed pairs (paced at the display rate), dropping
        // any at or past the terminal tick (so a replay isn't recorded past the
        // few ticks the simulator overshoots the end by).
        if let Some(observer) = self.observer.as_mut() {
            for i in 0..consumed {
                let tick = seed_tick + i as u32;
                if result.commit_before.is_some_and(|end| tick >= end) {
                    break;
                }
                observer.on_commit(tick, &self.settle_backlog[i]);
            }
        }
        for _ in 0..consumed {
            self.settle_backlog.pop_front();
        }

        self.settled_snapshot = Some(result.snapshot);
        Ok(())
    }

    /// Throwaway re-sim from the checkpoint to `target`, predicting the remote
    /// input across the range. Used only when `target > commit_frontier - 1`.
    /// Doesn't touch the checkpoint — the next settle re-processes the committed
    /// portion with real data.
    fn speculate_tail(&mut self, target: u32, peeked: &[W::Input]) -> Result<W::State, W::Error> {
        let seed_tick = self.settled_snapshot.as_ref().expect("settled state").tick;
        assert_eq!(
            seed_tick,
            self.commit_frontier.saturating_sub(1),
            "speculative tail seed must sit at the settled cap"
        );

        // Remote input for the speculative ticks: the predictor's guess from the
        // last confirmed remote, held constant across the tail.
        let predicted = self.predictor.predict(&self.last_committed_remote);
        let total = (target - seed_tick + 1) as usize;
        let mut input_pairs: Vec<Pair<W::Input>> = Vec::with_capacity(total);

        // First entry sits at the committed cap: real local+remote inputs from
        // the settle-backlog front (the simulator resolves its remote data
        // speculatively, since the next settle redoes it for real).
        input_pairs.push(self.settle_backlog[0].clone());
        // Trailing entries are pure speculation.
        for local in &peeked[..total - 1] {
            input_pairs.push(Pair {
                local: local.clone(),
                remote: predicted.clone(),
            });
        }

        let base = self.settled_snapshot.as_ref().expect("settled state");
        let result = self.simulator.simulate(base, input_pairs, true)?;
        Ok(result.snapshot.state)
    }

    fn update_slowdown(&mut self, presenter: &mut dyn Presenter<W>) {
        // Asymmetric time sync: only the leading peer slows. Both advantages
        // carry the symmetric network-delay term; their difference isolates
        // real-time clock skew.
        let local_advantage = self.local_frame_advantage() as i32;
        let remote_advantage = self.last_remote_frame_advantage as i32;
        let skew = local_advantage - remote_advantage;

        presenter.set_slowdown(self.throttler.step(skew));
    }
}
