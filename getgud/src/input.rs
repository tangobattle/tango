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

    /// How many ticks local input is ahead of remote input — i.e. the number of
    /// local inputs that currently have no matching remote input.
    ///
    /// This is the depth of the speculative window: how many future ticks must
    /// be simulated with *predicted* remote input to present the latest frame.
    pub fn speculative_depth(&self) -> usize {
        self.local_queue.len().saturating_sub(self.remote_queue.len())
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
