use std::collections::VecDeque;

/// One tick's local + remote input, paired for the simulator.
#[derive(Clone, Debug)]
pub struct Pair<Input> {
    pub local: Input,
    pub remote: Input,
}

/// The per-round delay queue. Local and remote inputs arrive independently;
/// [`consume_and_peek_local`](Queue::consume_and_peek_local) drains as many
/// matched pairs as both sides have, leaving the surplus local inputs as the
/// speculative window. The queue is unbounded — the host bounds how deep it
/// lets either side run by reading the queue lengths and stopping its own feed.
pub struct Queue<Input> {
    local_queue: VecDeque<Input>,
    remote_queue: VecDeque<Input>,
}

impl<Input> Queue<Input>
where
    Input: Clone,
{
    pub fn new() -> Self {
        Self {
            local_queue: VecDeque::new(),
            remote_queue: VecDeque::new(),
        }
    }

    pub fn add_local_input(&mut self, v: Input) {
        self.local_queue.push_back(v);
    }

    pub fn add_remote_input(&mut self, v: Input) {
        self.remote_queue.push_back(v);
    }

    pub fn local_queue_length(&self) -> usize {
        self.local_queue.len()
    }

    pub fn remote_queue_length(&self) -> usize {
        self.remote_queue.len()
    }

    pub fn speculative_depth(&self) -> usize {
        self.local_queue.len().saturating_sub(self.remote_queue.len())
    }

    pub fn consume_and_peek_local(&mut self) -> (Vec<Pair<Input>>, Vec<Input>) {
        let n = std::cmp::min(self.local_queue.len(), self.remote_queue.len());
        let to_commit = std::iter::zip(self.local_queue.drain(..n), self.remote_queue.drain(..n))
            .map(|(local, remote)| Pair { local, remote })
            .collect();

        // Everything still in the local queue is ahead of the latest remote
        // input — those frames get committed against a predicted remote input
        // until the real one arrives.
        let peeked = self.local_queue.iter().cloned().collect();

        (to_commit, peeked)
    }
}
