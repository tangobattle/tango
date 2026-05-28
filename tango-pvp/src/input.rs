/// A committed local-side input plus the matching outgoing packet for that
/// tick. Tick is positional — derived from the input's position in its
/// round / queue, never embedded in the struct.
#[derive(Clone, Debug)]
pub struct Input {
    pub joyflags: u16,
    pub packet: Vec<u8>,
}

/// A committed input without its outgoing packet. Local inputs upgrade to
/// `Input` once the Fastforwarder pairs them with a packet via
/// [`PartialInput::with_packet`].
#[derive(Clone, Debug)]
pub struct PartialInput {
    pub joyflags: u16,
}

impl PartialInput {
    pub fn with_packet(self, packet: Vec<u8>) -> Input {
        Input {
            joyflags: self.joyflags,
            packet,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Pair<LocalInput, RemoteInput> {
    pub local: LocalInput,
    pub remote: RemoteInput,
}

pub struct PairQueue<LocalInput, RemoteInput> {
    local_queue: std::collections::VecDeque<LocalInput>,
    remote_queue: std::collections::VecDeque<RemoteInput>,
    max_length: usize,
}

impl<LocalInput, RemoteInput> PairQueue<LocalInput, RemoteInput>
where
    LocalInput: Clone,
    RemoteInput: Clone,
{
    pub fn new(capacity: usize) -> Self {
        PairQueue {
            local_queue: std::collections::VecDeque::with_capacity(capacity),
            remote_queue: std::collections::VecDeque::with_capacity(capacity),
            max_length: capacity,
        }
    }

    pub fn add_local_input(&mut self, v: LocalInput) {
        self.local_queue.push_back(v);
    }

    pub fn can_add_local_input(&self) -> bool {
        self.local_queue.len() < self.max_length
    }

    pub fn add_remote_input(&mut self, v: RemoteInput) {
        self.remote_queue.push_back(v);
    }

    pub fn can_add_remote_input(&self) -> bool {
        self.remote_queue.len() < self.max_length
    }

    pub fn local_queue_length(&self) -> usize {
        self.local_queue.len()
    }

    pub fn remote_queue_length(&self) -> usize {
        self.remote_queue.len()
    }

    /// Local inputs queued past the latest remote — the speculative window.
    /// Invariant under `consume_and_peek_local` (which drains equal counts
    /// from both sides), so callers can read it at any point in the frame.
    pub fn speculative_depth(&self) -> usize {
        self.local_queue.len().saturating_sub(self.remote_queue.len())
    }

    pub fn consume_and_peek_local(&mut self) -> (Vec<Pair<LocalInput, RemoteInput>>, Vec<LocalInput>) {
        let n = std::cmp::min(self.local_queue.len(), self.remote_queue.len());
        let to_commit = std::iter::zip(self.local_queue.drain(..n), self.remote_queue.drain(..n))
            .map(|(local, remote)| Pair { local, remote })
            .collect();

        // Everything still in the local queue is ahead of the latest
        // remote input — those frames get committed against a predicted
        // remote input until the real one arrives.
        let peeked = self.local_queue.iter().cloned().collect();

        (to_commit, peeked)
    }
}
