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
/// frame, plus the metadata needed to render it and to keep the two peers'
/// clocks in sync.
pub struct Frame<'a, W: World> {
    /// The tick this frame represents (`frontier - present_delay`, clamped to
    /// what has been simulated). May be a speculative tick or a fully confirmed
    /// one depending on how far remote input has lagged.
    pub tick: u32,

    /// The clock-sync hint: local tick advantage minus the remote peer's
    /// reported tick advantage.
    ///
    /// Positive means this client is running ahead of the remote and should slow
    /// down (e.g. occasionally stall a frame) so the two simulations converge
    /// and the prediction window stays small. Zero means the peers are balanced.
    /// See [`Session::local_tick_advantage`].
    pub skew: i32,

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

/// A single peer's view of a two-player rollback session.
///
/// `Session` owns the local/remote input queues, the authoritative ("settled")
/// state, and the speculative state shown to the player. The intended loop is:
///
/// 1. As remote packets arrive, call [`add_remote_input`](Session::add_remote_input).
/// 2. Once per tick, call [`advance`](Session::advance) with the local input.
///    It confirms any newly-matched ticks into the settled state, speculates
///    forward with predicted remote input as needed, and returns a [`Frame`] to
///    render.
/// 3. Feed [`Frame::skew`] into your clock so the two peers stay aligned.
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

    commit_frontier: u32,
    last_committed_remote: W::Input,

    settle_backlog: VecDeque<(W::Input, W::Input)>,
    settled_state: W::State,
    settled_tick: u32,
    speculative_state: Option<W::State>,

    last_remote_received_tick: u32,
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
            settle_backlog: std::collections::VecDeque::new(),
            settled_state: initial_state,
            settled_tick: 0,
            speculative_state: None,
            last_remote_received_tick: 0,
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

    /// Advance the simulation by one local tick and return the [`Frame`] to
    /// present.
    ///
    /// This is the per-tick driver. It:
    ///
    /// 1. enqueues `local_input`;
    /// 2. matches it against any buffered remote inputs and folds the newly
    ///    confirmed ticks into the authoritative settled state (logging them);
    /// 3. computes the present target (`frontier - present_delay`);
    /// 4. if the target is beyond what's confirmed, simulates a throwaway
    ///    speculative tail using [`Predictor`]-supplied remote inputs;
    /// 5. returns the resulting state, tick, the local/remote input pair at that
    ///    tick, and clock [`skew`](Frame::skew).
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

        let settled_target = target.min(self.commit_frontier.saturating_sub(1));
        self.settle_to(settled_target)?;

        let skew = self.local_tick_advantage() as i32 - self.last_remote_tick_advantage as i32;

        self.frontier += 1;

        Ok(if target > settled_target && self.commit_frontier > 0 {
            let (state, input) = self.speculate_tail(target, &unmatched_locals)?;
            self.speculative_state = Some(state);
            Frame {
                tick: target,
                skew,
                state: self.speculative_state.as_ref().unwrap(),
                input,
            }
        } else {
            self.speculative_state = None;
            let input = self
                .settle_backlog
                .front()
                .cloned()
                .unwrap_or_else(|| {
                    (unmatched_locals[0].clone(), self.predictor.predict(&self.last_committed_remote))
                });
            Frame {
                tick: self.settled_tick,
                skew,
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

    /// How far the local frontier is ahead of the most recently received remote
    /// input, in ticks (saturated to [`i16`]).
    ///
    /// This is each peer's half of the clock-sync handshake: you send it to the
    /// remote with every input, and the remote's value comes back via
    /// [`add_remote_input`](Session::add_remote_input). The difference of the two
    /// is the [`skew`](Frame::skew) used to keep the simulations aligned.
    pub fn local_tick_advantage(&self) -> i16 {
        let diff = self.frontier as i32 - self.last_remote_received_tick as i32;
        diff.clamp(i16::MIN as i32, i16::MAX as i32) as i16
    }

    /// The tick advantage the remote peer last reported (via the
    /// `tick_advantage` argument to [`add_remote_input`](Session::add_remote_input)).
    pub fn last_remote_tick_advantage(&self) -> i16 {
        self.last_remote_tick_advantage
    }

    /// How many ticks of prediction the latest frame requires — the count of
    /// local inputs with no confirmed remote counterpart yet.
    pub fn speculative_depth(&self) -> u32 {
        self.input_queue.speculative_depth() as u32
    }

    /// Record an input received from the remote peer.
    ///
    /// * `input` — the remote player's input for the next unmatched tick.
    /// * `tick_advantage` — the remote peer's reported
    ///   [`local_tick_advantage`](Session::local_tick_advantage), used to
    ///   compute clock [`skew`](Frame::skew).
    ///
    /// Call this whenever remote inputs arrive; they are matched to local inputs
    /// on the next [`advance`](Session::advance).
    pub fn add_remote_input(&mut self, input: W::Input, tick_advantage: i16) {
        self.input_queue.add_remote_input(input);
        self.last_remote_received_tick = self.last_remote_received_tick.wrapping_add(1);
        self.last_remote_tick_advantage = tick_advantage;
    }

    /// Authoritatively advance the settled state up to `target` by consuming
    /// confirmed pairs from the backlog, logging them, and updating
    /// `last_committed_remote` (the prediction seed). No-op if already settled to
    /// or past `target`.
    fn settle_to(&mut self, target: u32) -> Result<(), W::Error> {
        let seed_tick = self.settled_tick;
        if target <= seed_tick {
            return Ok(());
        }

        let consumed = (target - seed_tick) as usize;
        assert!(self.settle_backlog.len() > consumed);
        let inputs: Vec<(W::Input, W::Input)> = self.settle_backlog.iter().take(consumed).cloned().collect();

        let result = self.simulator.simulate(&self.settled_state, seed_tick, inputs, false)?;

        let (_local, remote) = &self.settle_backlog[consumed - 1];
        self.last_committed_remote = remote.clone();

        for i in 0..result.committed.min(consumed) {
            self.logger.log(&self.settle_backlog[i]);
        }
        for _ in 0..consumed {
            self.settle_backlog.pop_front();
        }

        self.settled_state = result.state;
        self.settled_tick = target;
        Ok(())
    }

    /// Simulate a throwaway tail from the settled cap up to `target`, using real
    /// local inputs and predicted remote inputs, and return the speculative
    /// state plus the `(local, predicted-remote)` input pair at `target`. Rebuilt
    /// from scratch each frame, so mispredictions self-correct as real remote
    /// inputs get settled.
    fn speculate_tail(&mut self, target: u32, unmatched_locals: &[W::Input]) -> Result<(W::State, (W::Input, W::Input)), W::Error> {
        let seed_tick = self.settled_tick;
        assert_eq!(
            seed_tick,
            self.commit_frontier.saturating_sub(1),
            "speculative tail seed must sit at the settled cap"
        );

        let predicted = self.predictor.predict(&self.last_committed_remote);
        let input_count = (target - seed_tick) as usize;
        let mut inputs: Vec<(W::Input, W::Input)> = Vec::with_capacity(input_count);

        inputs.push(self.settle_backlog[0].clone());
        for local in &unmatched_locals[..input_count - 1] {
            inputs.push((local.clone(), predicted.clone()));
        }
        let local_input = unmatched_locals[input_count - 1].clone();

        let result = self.simulator.simulate(&self.settled_state, seed_tick, inputs, true)?;
        Ok((result.state, (local_input, predicted)))
    }
}
