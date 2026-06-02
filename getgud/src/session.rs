use std::collections::VecDeque;
use std::sync::Arc;

use crate::input::Queue;
use crate::present::Presenter;
use crate::sim::{CommitObserver, Predictor, Simulator};
use crate::world::{Snapshot, World};

/// Everything needed to construct a [`Session`]. Pass to [`Session::new`].
pub struct SessionParams<W: World> {
    /// Ticks the display lags the frontier (`target = frontier - frame_delay`).
    /// A small input buffer: larger values mean fewer rollbacks but more input
    /// latency. Adjustable later via [`Session::set_frame_delay`].
    pub frame_delay: u32,
    /// Seed remote input used for the very first prediction, before any real
    /// remote input has arrived.
    pub initial_remote: W::Input,
    /// Advances the world (authoritative commits and throwaway tails).
    pub simulator: Box<dyn Simulator<W>>,
    /// Guesses remote inputs for the speculative tail.
    pub predictor: Arc<dyn Predictor<W>>,
    /// Optional hook over confirmed history (e.g. replay recording).
    pub observer: Option<Box<dyn CommitObserver<W>>>,
}

/// The rollback engine for one local + one remote participant.
///
/// Holds the authoritative settled checkpoint and the input queues, and on each
/// [`advance`](Session::advance) settles confirmed inputs into the checkpoint,
/// re-simulates a predicted tail up to the display target, and presents it.
///
/// Lifecycle: [`new`](Session::new) → [`set_first_settled_state`](Session::set_first_settled_state)
/// (seed tick 0) → per frame, [`advance_frontier`](Session::advance_frontier)
/// then [`advance`](Session::advance); feed remote inputs in with
/// [`add_remote_input`](Session::add_remote_input) as they arrive.
pub struct Session<W: World> {
    frame_delay: u32,

    simulator: Box<dyn Simulator<W>>,
    predictor: Arc<dyn Predictor<W>>,
    observer: Option<Box<dyn CommitObserver<W>>>,

    frontier: u32,
    presented_tick: u32,

    input_queue: Queue<W::Input>,

    commit_frontier: u32,
    last_committed_remote: W::Input,

    settle_backlog: VecDeque<(W::Input, W::Input)>,
    settled_snapshot: Option<Snapshot<W>>,

    last_remote_received_tick: u32,
    last_remote_frame_advantage: i16,
}

impl<W: World> Session<W> {
    /// Build a session from [`SessionParams`]. Call
    /// [`set_first_settled_state`](Self::set_first_settled_state) before the
    /// first [`advance`](Self::advance).
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
        }
    }

    /// The local wall-clock tick counter.
    pub fn frontier(&self) -> u32 {
        self.frontier
    }

    /// The tick actually drawn during the last [`advance`](Self::advance).
    /// Usually `frontier - frame_delay`, but clamped early in a match and when
    /// `frame_delay` changes live, so read it rather than recomputing.
    pub fn presented_tick(&self) -> u32 {
        self.presented_tick
    }

    /// The current display lag, in ticks.
    pub fn frame_delay(&self) -> u32 {
        self.frame_delay
    }

    /// Adjust the display lag live. Takes effect on the next
    /// [`advance`](Self::advance).
    pub fn set_frame_delay(&mut self, frame_delay: u32) {
        self.frame_delay = frame_delay;
    }

    /// Advance the wall clock by one tick. Call once per rendered frame, before
    /// [`advance`](Self::advance).
    pub fn advance_frontier(&mut self) {
        self.frontier += 1;
    }

    /// Seed the authoritative checkpoint with the world state at tick 0. Must be
    /// called once before the first [`advance`](Self::advance).
    pub fn set_first_settled_state(&mut self, state: W::State) {
        self.settled_snapshot = Some(Snapshot { state, tick: 0 });
    }

    /// Whether [`set_first_settled_state`](Self::set_first_settled_state) has
    /// been called.
    pub fn has_settled_snapshot(&self) -> bool {
        self.settled_snapshot.is_some()
    }

    /// Run one frame: append `local_input`, match any newly pairable inputs,
    /// settle the checkpoint as far as confirmed inputs allow, then present
    /// either a speculative tail (predicted remotes) or the checkpoint itself
    /// via `presenter`, handing it the current time-sync skew.
    ///
    /// Call once per rendered frame, after [`advance_frontier`](Self::advance_frontier)
    /// and after [`set_first_settled_state`](Self::set_first_settled_state).
    /// Propagates any [`W::Error`](World::Error) from the simulator.
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

        // Raw time-sync skew handed to the presenter's own throttle. Both
        // advantages carry the symmetric network delay, so their difference
        // isolates the real clock skew between the two peers; positive means
        // we're running ahead and should ease off. Computed up front so the
        // borrow ends before we touch `settled_snapshot` below.
        let skew = self.local_frame_advantage() as i32 - self.last_remote_frame_advantage as i32;

        let tick = if target > settled_target && self.commit_frontier > 0 {
            // Speculative tail: throwaway re-sim from the checkpoint to the
            // target, predicting every remote payload past the frontier.
            let state = self.speculate_tail(target, &peeked)?;
            presenter.present(&state, skew);
            target
        } else {
            // Settled (or pre-first-commit): the checkpoint IS the display. Its
            // tick is not always `target` — the wall clock can push `target`
            // past tick 0 before the first remote input lands, and a live
            // frame_delay increase can pull it back behind the cap — so track
            // the tick actually presented.
            let snapshot = self.settled_snapshot.as_ref().unwrap();
            presenter.present(&snapshot.state, skew);
            snapshot.tick
        };
        self.presented_tick = tick;
        Ok(())
    }

    /// Local inputs not yet matched into a confirmed pair.
    pub fn local_queue_length(&self) -> usize {
        self.input_queue.local_queue_length()
    }

    /// Remote inputs not yet matched into a confirmed pair.
    pub fn remote_queue_length(&self) -> usize {
        self.input_queue.remote_queue_length()
    }

    /// How far the local frontier leads the remote inputs received so far.
    /// Send this to the peer each frame so its throttler can sync against you.
    pub fn local_frame_advantage(&self) -> i16 {
        let diff = self.frontier as i32 - self.last_remote_received_tick as i32;
        diff.clamp(i16::MIN as i32, i16::MAX as i32) as i16
    }

    /// The lead the peer reported with its most recent remote input (see
    /// [`add_remote_input`](Self::add_remote_input)).
    pub fn last_remote_frame_advantage(&self) -> i16 {
        self.last_remote_frame_advantage
    }

    /// Local frames currently being simulated against a predicted remote.
    pub fn speculative_depth(&self) -> u32 {
        self.input_queue.speculative_depth() as u32
    }

    /// Feed in a remote input as it arrives off the transport, along with the
    /// `frame_advantage` the peer reported (its [`local_frame_advantage`](Self::local_frame_advantage)),
    /// used to drive time synchronization.
    pub fn add_remote_input(&mut self, input: W::Input, frame_advantage: i16) {
        self.input_queue.add_remote_input(input);
        self.last_remote_received_tick = self.last_remote_received_tick.wrapping_add(1);
        self.last_remote_frame_advantage = frame_advantage;
    }

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
        let input_pairs: Vec<(W::Input, W::Input)> = self.settle_backlog.iter().take(count).cloned().collect();

        let result = self.simulator.simulate(&seed, input_pairs, false)?;

        // The last settled pair's remote seeds the speculative tail's
        // prediction. `consumed >= 1` here (we returned early if `target <=
        // seed_tick`), so the index is in range.
        let consumed = (target - seed_tick) as usize;
        let (_local, remote) = &self.settle_backlog[consumed - 1];
        self.last_committed_remote = remote.clone();

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
        let mut input_pairs: Vec<(W::Input, W::Input)> = Vec::with_capacity(total);

        // First entry sits at the committed cap: real local+remote inputs from
        // the settle-backlog front (the simulator resolves its remote data
        // speculatively, since the next settle redoes it for real).
        input_pairs.push(self.settle_backlog[0].clone());
        // Trailing entries are pure speculation.
        for local in &peeked[..total - 1] {
            input_pairs.push((local.clone(), predicted.clone()));
        }

        let base = self.settled_snapshot.as_ref().expect("settled state");
        let result = self.simulator.simulate(base, input_pairs, true)?;
        Ok(result.snapshot.state)
    }
}
