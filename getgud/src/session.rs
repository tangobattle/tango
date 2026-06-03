use std::collections::VecDeque;
use std::sync::Arc;

use crate::input::Queue;
use crate::sim::{Logger, Predictor, Simulator};
use crate::world::{Snapshot, World};

/// Everything needed to construct a [`Session`]. Pass to [`Session::new`].
pub struct SessionParams<W: World> {
    /// Ticks the display lags the frontier (`target = frontier - present_delay`).
    /// A small input buffer: larger values mean fewer rollbacks but more input
    /// latency. Adjustable later via [`Session::set_present_delay`].
    pub present_delay: u32,
    /// Seed remote input used for the very first prediction, before any real
    /// remote input has arrived.
    pub initial_remote: W::Input,
    /// The world state at tick 0. Seeds the settled checkpoint at construction,
    /// so the session always holds an authoritative state — there is no separate
    /// "seed me later" step.
    pub initial_state: W::State,
    /// Advances the world (authoritative commits and throwaway tails).
    pub simulator: Box<dyn Simulator<W>>,
    /// Guesses remote inputs for the speculative tail.
    pub predictor: Arc<dyn Predictor<W>>,
    /// Optional hook over confirmed history (e.g. replay recording).
    pub logger: Box<dyn Logger<W>>,
}

/// The state to display this tick, returned by [`Session::advance`].
pub struct Frame<'a, W: World> {
    /// The simulation tick `state` represents. Usually `frontier - present_delay`,
    /// but clamped early in a match and when `present_delay` changes live, so read
    /// it rather than recomputing.
    pub tick: u32,
    /// The raw time-sync skew in ticks, `local_tick_advantage -
    /// last_remote_tick_advantage`. Both advantages carry the symmetric network
    /// delay, so their difference isolates the real clock skew between the two
    /// peers; positive means we're running ahead and should ease off. Feed it to
    /// your own throttle.
    pub skew: i32,
    /// The world state to draw — a borrow into the session, valid only until the
    /// next [`advance`](Session::advance). Usually a speculative (predicted) view
    /// that is recomputed each tick; don't retain it.
    pub state: &'a W::State,
    /// The local input poised at [`tick`](Self::tick): sampled but not yet
    /// stepped — the start-of-tick input the snapshot invariant leaves un-applied
    /// (see [`Simulator`](crate::Simulator)). A consumer that bakes the boundary
    /// input onto the displayed state (e.g. priming an input register the resumed
    /// core will read) applies this to `state` itself.
    ///
    /// Always present: every displayed tick has a local input. Even the
    /// pre-first-commit bootstrap frame (whose `state` is the caller's
    /// [`initial_state`](crate::SessionParams::initial_state) at tick 0) carries
    /// tick 0's local input, so the consumer never has to special-case it.
    pub local_input: W::Input,
}

/// The rollback engine for one local + one remote participant.
///
/// Holds the authoritative settled checkpoint and the input queues, and on each
/// [`advance`](Session::advance) settles confirmed inputs into the checkpoint
/// and re-simulates a predicted tail up to the display target, returning the
/// [`Frame`] to draw.
///
/// Lifecycle: [`new`](Session::new) (seeds tick 0 from
/// [`SessionParams::initial_state`]) → call [`advance`](Session::advance) once
/// per tick (it advances the wall clock itself); feed remote inputs in with
/// [`add_remote_input`](Session::add_remote_input) as they arrive.
pub struct Session<W: World> {
    present_delay: u32,

    simulator: Box<dyn Simulator<W>>,
    predictor: Arc<dyn Predictor<W>>,
    logger: Box<dyn Logger<W>>,

    frontier: u32,

    input_queue: Queue<W::Input>,

    commit_frontier: u32,
    last_committed_remote: W::Input,

    settle_backlog: VecDeque<(W::Input, W::Input)>,
    settled_snapshot: Snapshot<W>,
    /// The speculative tail's state from the most recent [`advance`](Self::advance),
    /// retained only so `advance` can hand back a borrow instead of cloning. It
    /// is overwritten on a speculative tick and cleared on a settled one.
    speculative_state: Option<W::State>,

    last_remote_received_tick: u32,
    last_remote_tick_advantage: i16,
}

impl<W: World> Session<W> {
    /// Build a session from [`SessionParams`]. The settled checkpoint is seeded
    /// at tick 0 from [`SessionParams::initial_state`], so it is ready to
    /// [`advance`](Self::advance) immediately.
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
            settle_backlog: std::collections::VecDeque::new(),
            settled_snapshot: Snapshot {
                state: initial_state,
                tick: 0,
            },
            speculative_state: None,
            last_remote_received_tick: 0,
            last_remote_tick_advantage: 0,
        }
    }

    /// The local wall-clock tick counter.
    pub fn frontier(&self) -> u32 {
        self.frontier
    }

    /// The current display lag, in ticks.
    pub fn present_delay(&self) -> u32 {
        self.present_delay
    }

    /// Adjust the display lag live. Takes effect on the next
    /// [`advance`](Self::advance).
    pub fn set_present_delay(&mut self, present_delay: u32) {
        self.present_delay = present_delay;
    }

    /// Run one tick: advance the wall clock, append `local_input`, match any
    /// newly pairable inputs, settle the checkpoint as far as confirmed inputs
    /// allow, then return the [`Frame`] to draw — either a speculative tail
    /// (predicted remotes) or the checkpoint itself — carrying the time-sync
    /// [`skew`](Frame::skew) to drive your own throttle.
    ///
    /// Call once per rendered tick. Propagates any [`W::Error`](World::Error)
    /// from the simulator.
    pub fn advance(&mut self, local_input: W::Input) -> Result<Frame<'_, W>, W::Error> {
        self.input_queue.add_local_input(local_input);

        // Drain matched pairs onto the commit chain.
        let (committable, unmatched_locals) = self.input_queue.drain_matched();
        self.commit_frontier += committable.len() as u32;
        self.settle_backlog.extend(committable);

        // Display target: what the user should see this wall-tick.
        let target = self.frontier.saturating_sub(self.present_delay);

        // Settle forward, capped at the last confirmed tick — the checkpoint
        // must never absorb predicted payloads.
        let settled_target = target.min(self.commit_frontier.saturating_sub(1));
        self.settle_to(settled_target)?;

        // Snapshot the skew before bumping the frontier below, so it reflects
        // this tick's lead rather than the next one's.
        let skew = self.local_tick_advantage() as i32 - self.last_remote_tick_advantage as i32;

        // This call IS the per-tick wall clock: bump the frontier now that this
        // tick's work is done, before borrowing `self` for the returned frame.
        self.frontier += 1;

        Ok(if target > settled_target && self.commit_frontier > 0 {
            // Speculative tail: throwaway re-sim from the checkpoint to the
            // target, predicting every remote payload past the frontier. Stash
            // it so we can return a borrow without cloning.
            let (state, local_input) = self.speculate_tail(target, &unmatched_locals)?;
            self.speculative_state = Some(state);
            Frame {
                tick: target,
                skew,
                state: self.speculative_state.as_ref().unwrap(),
                local_input,
            }
        } else {
            // Settled (or pre-first-commit): the checkpoint IS the display. Its
            // tick is not always `target` — the wall clock can push `target`
            // past tick 0 before the first remote input lands, and a live
            // present_delay increase can pull it back behind the cap — so report
            // the tick actually presented.
            self.speculative_state = None;
            // The local input poised at the displayed tick. Normally the front of
            // the settle backlog — the next pair to commit. Before the first
            // commit the backlog is empty and the display is the initial state at
            // tick 0, whose local input is the oldest still-unmatched local
            // (`commit_frontier == 0` means nothing has drained, so it exists).
            let local_input = self
                .settle_backlog
                .front()
                .map(|(local, _remote)| local.clone())
                .unwrap_or_else(|| unmatched_locals[0].clone());
            Frame {
                tick: self.settled_snapshot.tick,
                skew,
                state: &self.settled_snapshot.state,
                local_input,
            }
        })
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
    /// Send this to the peer each tick so its throttler can sync against you.
    pub fn local_tick_advantage(&self) -> i16 {
        let diff = self.frontier as i32 - self.last_remote_received_tick as i32;
        diff.clamp(i16::MIN as i32, i16::MAX as i32) as i16
    }

    /// The lead the peer reported with its most recent remote input (see
    /// [`add_remote_input`](Self::add_remote_input)).
    pub fn last_remote_tick_advantage(&self) -> i16 {
        self.last_remote_tick_advantage
    }

    /// Local ticks currently being simulated against a predicted remote.
    pub fn speculative_depth(&self) -> u32 {
        self.input_queue.speculative_depth() as u32
    }

    /// Feed in a remote input as it arrives off the transport, along with the
    /// `tick_advantage` the peer reported (its [`local_tick_advantage`](Self::local_tick_advantage)),
    /// used to drive time synchronization.
    pub fn add_remote_input(&mut self, input: W::Input, tick_advantage: i16) {
        self.input_queue.add_remote_input(input);
        self.last_remote_received_tick = self.last_remote_received_tick.wrapping_add(1);
        self.last_remote_tick_advantage = tick_advantage;
    }

    fn settle_to(&mut self, target: u32) -> Result<(), W::Error> {
        let seed_tick = self.settled_snapshot.tick;
        if target <= seed_tick {
            return Ok(());
        }

        // Commit the `[seed_tick, target)` pairs: the simulator advances through
        // every committed pair and leaves the snapshot poised at the start of
        // `target`. The input at `target` is not stepped here — it rides out on
        // the next `advance`'s `Frame::local_input`. The `target <=
        // commit_frontier - 1` cap keeps at least one confirmed pair past the
        // committed ones in the backlog, so the settled frame's `local_input`
        // and the speculative tail both have a real pair to seed from.
        let consumed = (target - seed_tick) as usize;
        assert!(self.settle_backlog.len() > consumed);
        let inputs: Vec<(W::Input, W::Input)> = self.settle_backlog.iter().take(consumed).cloned().collect();

        let result = self.simulator.simulate(&self.settled_snapshot, inputs, false)?;

        // The last committed pair's remote seeds the speculative tail's
        // prediction. `consumed >= 1` here (we returned early if `target <=
        // seed_tick`), so the index is in range.
        let (_local, remote) = &self.settle_backlog[consumed - 1];
        self.last_committed_remote = remote.clone();

        // Report the just-committed pairs (paced at the display rate), dropping
        // any the simulator didn't consume before terminating (so a replay isn't
        // recorded past the few ticks the simulator overshoots the end by).
        for i in 0..result.committed.min(consumed) {
            self.logger.log(&self.settle_backlog[i]);
        }
        for _ in 0..consumed {
            self.settle_backlog.pop_front();
        }

        self.settled_snapshot = result.snapshot;
        Ok(())
    }

    fn speculate_tail(
        &mut self,
        target: u32,
        unmatched_locals: &[W::Input],
    ) -> Result<(W::State, W::Input), W::Error> {
        let seed_tick = self.settled_snapshot.tick;
        assert_eq!(
            seed_tick,
            self.commit_frontier.saturating_sub(1),
            "speculative tail seed must sit at the settled cap"
        );

        // Remote input for the speculative ticks: the predictor's guess from the
        // last confirmed remote, held constant across the tail.
        let predicted = self.predictor.predict(&self.last_committed_remote);
        let input_count = (target - seed_tick) as usize;
        let mut inputs: Vec<(W::Input, W::Input)> = Vec::with_capacity(input_count);

        // First applied entry sits at the committed cap: real local+remote
        // inputs from the settle-backlog front (the simulator resolves its remote
        // data speculatively, since the next settle redoes it for real).
        inputs.push(self.settle_backlog[0].clone());
        // The remaining applied entries are pure speculation.
        for local in &unmatched_locals[..input_count - 1] {
            inputs.push((local.clone(), predicted.clone()));
        }
        // The local input one tick past the last applied — sampled but not
        // stepped — rides out on the Frame so the consumer can prime it onto the
        // displayed state.
        let local_input = unmatched_locals[input_count - 1].clone();

        let result = self.simulator.simulate(&self.settled_snapshot, inputs, true)?;
        Ok((result.snapshot.state, local_input))
    }
}
