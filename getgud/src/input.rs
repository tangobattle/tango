use std::collections::VecDeque;

/// Parallel FIFO buffers — one for local inputs, one per remote peer —
/// that the session pairs up tick-by-tick.
///
/// Local inputs are produced every frame; remote inputs arrive asynchronously
/// over the network and generally lag behind. A tick is only *confirmed* once
/// every player's input for it is present, which is exactly what
/// [`drain_matched`](Queue::drain_matched) extracts. Anything still unmatched
/// is an input awaiting the stragglers — the raw material for speculation.
///
/// You normally don't construct a `Queue` directly; [`Session`](crate::Session)
/// owns one. It is exposed for testing and inspection.
pub struct Queue<Input> {
    local_queue: VecDeque<Input>,
    remote_queues: Vec<VecDeque<Input>>,
}

impl<Input> Queue<Input>
where
    Input: Clone,
{
    /// Create an empty queue for one local player and `num_remotes` remote
    /// peers.
    pub fn new(num_remotes: usize) -> Self {
        assert!(num_remotes >= 1, "a session needs at least one remote");
        Self {
            local_queue: VecDeque::new(),
            remote_queues: (0..num_remotes).map(|_| VecDeque::new()).collect(),
        }
    }

    /// Number of remote peers this queue tracks.
    pub fn num_remotes(&self) -> usize {
        self.remote_queues.len()
    }

    /// Append a locally produced input to the back of the local stream.
    pub fn add_local_input(&mut self, v: Input) {
        self.local_queue.push_back(v);
    }

    /// Append an input received from remote peer `remote` to the back of
    /// that peer's stream.
    pub fn add_remote_input(&mut self, remote: usize, v: Input) {
        self.remote_queues[remote].push_back(v);
    }

    /// Number of local inputs still buffered.
    pub fn local_queue_length(&self) -> usize {
        self.local_queue.len()
    }

    /// Number of inputs from remote peer `remote` still buffered.
    pub fn remote_queue_length(&self, remote: usize) -> usize {
        self.remote_queues[remote].len()
    }

    /// The shortest remote input backlog — how many *fully*-matched rows a
    /// [`drain_matched`](Queue::drain_matched) could commit right now (a tick
    /// is only confirmable once every remote's input for it has arrived, so
    /// the count is bounded by the furthest-behind remote). Zero means no
    /// confirmable progress is available without more remote input. Used as
    /// the stall guard's "can we still drain?" signal: a full local queue that
    /// can still be matched against buffered remote inputs must keep advancing,
    /// or the drain never happens.
    pub fn matchable(&self) -> usize {
        self.remote_queues.iter().map(|q| q.len()).min().unwrap_or(0)
    }

    /// How many ticks local input leads remote peer `remote`'s input,
    /// signed: positive when the local queue is longer (local ahead of that
    /// remote), negative when that peer's received inputs have piled up past
    /// the local ones (local behind).
    pub fn lead_over(&self, remote: usize) -> i32 {
        self.local_queue.len() as i32 - self.remote_queues[remote].len() as i32
    }

    /// The worst-case lead: [`lead_over`](Queue::lead_over) the
    /// furthest-behind remote. This is what bounds speculation — a tick is
    /// only confirmed once *every* remote's input has arrived.
    ///
    /// This is the *raw* lead, not the speculation depth, and it can go
    /// negative — callers that want only the non-negative lead (e.g.
    /// speculation math) take `.max(0)`. The queue has no notion of present
    /// delay, which buffers the first `present_delay` ticks of the lead
    /// before any of it has to be rendered speculatively; only the excess
    /// is. See [`Session::speculation_balance`](crate::Session::speculation_balance)
    /// for the depth actually speculated, and
    /// [`Session::local_tick_advantage`](crate::Session::local_tick_advantage),
    /// which surfaces this as one half of the clock-sync skew.
    pub fn lead(&self) -> i32 {
        (0..self.remote_queues.len())
            .map(|i| self.lead_over(i))
            .max()
            .expect("queue has at least one remote")
    }

    /// Pull every tick for which all players' inputs are now present.
    ///
    /// Returns `(matched, unmatched_locals, unmatched_remotes)`:
    ///
    /// * `matched` — `(local, remotes)` rows for confirmed ticks, in order,
    ///   removed from every queue. `remotes` is indexed by remote slot.
    /// * `unmatched_locals` — a clone of the local inputs left over after
    ///   matching (those still waiting on at least one remote). These remain
    ///   in the local queue.
    /// * `unmatched_remotes` — per remote slot, a clone of that peer's
    ///   inputs left over after matching: real inputs for ticks that are not
    ///   yet fully confirmed because *another* remote is still missing.
    ///   These remain in their queues, and let speculation use a real input
    ///   where one has already arrived.
    #[allow(clippy::type_complexity)]
    pub fn drain_matched(&mut self) -> (Vec<(Input, Box<[Input]>)>, Vec<Input>, Vec<Vec<Input>>) {
        let n = self
            .remote_queues
            .iter()
            .map(|q| q.len())
            .min()
            .expect("queue has at least one remote")
            .min(self.local_queue.len());

        let mut rows: Vec<(Input, Vec<Input>)> = self
            .local_queue
            .drain(..n)
            .map(|l| (l, Vec::with_capacity(self.remote_queues.len())))
            .collect();
        for q in &mut self.remote_queues {
            for (row, v) in rows.iter_mut().zip(q.drain(..n)) {
                row.1.push(v);
            }
        }
        let matched = rows.into_iter().map(|(l, r)| (l, r.into_boxed_slice())).collect();

        let unmatched_locals = self.local_queue.iter().cloned().collect();
        let unmatched_remotes = self.remote_queues.iter().map(|q| q.iter().cloned().collect()).collect();

        (matched, unmatched_locals, unmatched_remotes)
    }
}
