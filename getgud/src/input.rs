use std::collections::VecDeque;

/// The pair of FIFO input streams the session matches into `(local, remote)`
/// tuples.
///
/// Local inputs are produced every tick; remote inputs arrive off the network.
/// Inputs at the same depth in both queues form a confirmed pair; local inputs
/// beyond the last received remote are the speculative frontier.
pub struct Queue<Input> {
    local_queue: VecDeque<Input>,
    remote_queue: VecDeque<Input>,
}

impl<Input> Queue<Input>
where
    Input: Clone,
{
    /// An empty queue.
    pub fn new() -> Self {
        Self {
            local_queue: VecDeque::new(),
            remote_queue: VecDeque::new(),
        }
    }

    /// Enqueue a local input (one per tick).
    pub fn add_local_input(&mut self, v: Input) {
        self.local_queue.push_back(v);
    }

    /// Enqueue a remote input as it arrives off the transport.
    pub fn add_remote_input(&mut self, v: Input) {
        self.remote_queue.push_back(v);
    }

    /// Number of local inputs not yet matched into a pair.
    pub fn local_queue_length(&self) -> usize {
        self.local_queue.len()
    }

    /// Number of remote inputs not yet matched into a pair.
    pub fn remote_queue_length(&self) -> usize {
        self.remote_queue.len()
    }

    /// How many local ticks are ahead of the latest remote input — i.e. how
    /// many ticks currently run on a predicted remote.
    pub fn speculative_depth(&self) -> usize {
        self.local_queue.len().saturating_sub(self.remote_queue.len())
    }

    /// Drain every local/remote input that now has a counterpart into confirmed
    /// pairs, and return them alongside a clone of the still-unmatched local
    /// inputs.
    ///
    /// The unmatched locals (the second tuple element) are the ticks ahead of
    /// the latest remote input; they get simulated against a *predicted* remote
    /// until the real one arrives.
    pub fn drain_matched(&mut self) -> (Vec<(Input, Input)>, Vec<Input>) {
        let n = std::cmp::min(self.local_queue.len(), self.remote_queue.len());
        let to_commit = std::iter::zip(self.local_queue.drain(..n), self.remote_queue.drain(..n)).collect();

        // Everything still in the local queue is ahead of the latest remote
        // input — those ticks get committed against a predicted remote input
        // until the real one arrives.
        let unmatched_locals = self.local_queue.iter().cloned().collect();

        (to_commit, unmatched_locals)
    }
}
