use std::collections::VecDeque;

/// Two parallel FIFO buffers — one for local inputs, one for remote inputs —
/// that the session pairs up tick-by-tick.
///
/// Local inputs are produced every frame; remote inputs arrive asynchronously
/// over the network and generally lag behind. A tick is only *confirmed* once
/// both players' inputs for it are present, which is exactly what
/// [`drain_matched`](Queue::drain_matched) extracts. Anything still unmatched is
/// a local input awaiting its remote counterpart — the raw material for
/// speculation.
///
/// You normally don't construct a `Queue` directly; [`Session`](crate::Session)
/// owns one. It is exposed for testing and inspection.
pub struct Queue<Input> {
    local_queue: VecDeque<Input>,
    remote_queue: VecDeque<Input>,
}

impl<Input> Queue<Input>
where
    Input: Clone,
{
    /// Create an empty queue.
    pub fn new() -> Self {
        Self {
            local_queue: VecDeque::new(),
            remote_queue: VecDeque::new(),
        }
    }

    /// Append a locally produced input to the back of the local stream.
    pub fn add_local_input(&mut self, v: Input) {
        self.local_queue.push_back(v);
    }

    /// Append an input received from the remote peer to the back of the remote
    /// stream.
    pub fn add_remote_input(&mut self, v: Input) {
        self.remote_queue.push_back(v);
    }

    /// Number of local inputs still buffered.
    pub fn local_queue_length(&self) -> usize {
        self.local_queue.len()
    }

    /// Number of remote inputs still buffered.
    pub fn remote_queue_length(&self) -> usize {
        self.remote_queue.len()
    }

    /// How many ticks local input leads remote input, signed: positive when the
    /// local queue is longer (local ahead of remote), negative when received
    /// remote inputs have piled up past the local ones (local behind).
    ///
    /// This is the *raw* lead, not the speculation depth, and it can go
    /// negative — callers that want only the non-negative lead (e.g. speculation
    /// math) take `.max(0)`. The queue has no notion of present delay, which
    /// buffers the first `present_delay` ticks of the lead before any of it has
    /// to be rendered speculatively; only the excess is. See
    /// [`Session::speculation_balance`](crate::Session::speculation_balance) for
    /// the depth actually speculated, and
    /// [`Session::local_tick_advantage`](crate::Session::local_tick_advantage),
    /// which surfaces this as one half of the clock-sync skew.
    pub fn lead(&self) -> i32 {
        self.local_queue.len() as i32 - self.remote_queue.len() as i32
    }

    /// Pull every tick for which both players' inputs are now present.
    ///
    /// Returns `(matched, unmatched_locals)`:
    ///
    /// * `matched` — `(local, remote)` pairs for confirmed ticks, in order,
    ///   removed from both queues.
    /// * `unmatched_locals` — a clone of the local inputs left over after
    ///   matching (those still waiting on a remote input). These remain in the
    ///   local queue.
    pub fn drain_matched(&mut self) -> (Vec<(Input, Input)>, Vec<Input>) {
        let n = std::cmp::min(self.local_queue.len(), self.remote_queue.len());
        let to_commit = std::iter::zip(self.local_queue.drain(..n), self.remote_queue.drain(..n)).collect();

        let unmatched_locals = self.local_queue.iter().cloned().collect();

        (to_commit, unmatched_locals)
    }
}
