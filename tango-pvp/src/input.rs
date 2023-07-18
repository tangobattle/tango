#[derive(Clone)]
pub struct Packet {
    pub packet: Vec<u8>,
    pub tick: u32,
}

#[derive(Clone, Debug)]
pub struct Input {
    pub local_tick: u32,
    pub remote_tick: u32,
    pub joyflags: u16,
    pub packet: Vec<u8>,
}

impl Input {
    pub fn lag(&self) -> i32 {
        self.remote_tick as i32 - self.local_tick as i32
    }
}

#[derive(Clone, Debug)]
pub struct PartialInput {
    pub local_tick: u32,
    pub remote_tick: u32,
    pub joyflags: u16,
}

impl PartialInput {
    pub fn lag(&self) -> i32 {
        self.remote_tick as i32 - self.local_tick as i32
    }

    pub fn with_packet(self, packet: Vec<u8>) -> Input {
        Input {
            local_tick: self.local_tick,
            remote_tick: self.remote_tick,
            joyflags: self.joyflags,
            packet,
        }
    }
}

pub struct PairQueue<LocalInput, RemoteInput> {
    local_queue: std::collections::VecDeque<LocalInput>,
    remote_queue: std::collections::VecDeque<RemoteInput>,
    local_delay: u32,
    max_length: usize,
}

#[derive(Clone, Debug)]
pub struct Pair<LocalInput, RemoteInput> {
    pub local: LocalInput,
    pub remote: RemoteInput,
}

impl<LocalInput, RemoteInput> PairQueue<LocalInput, RemoteInput>
where
    LocalInput: Clone,
    RemoteInput: Clone,
{
    pub fn new(capacity: usize, local_delay: u32) -> Self {
        PairQueue {
            local_queue: std::collections::VecDeque::with_capacity(capacity),
            remote_queue: std::collections::VecDeque::with_capacity(capacity),
            local_delay,
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

    pub fn local_delay(&self) -> u32 {
        self.local_delay
    }

    pub fn local_queue_length(&self) -> usize {
        self.local_queue.len()
    }

    pub fn remote_queue_length(&self) -> usize {
        self.remote_queue.len()
    }

    pub fn consume_and_peek_local(&mut self) -> (Vec<Pair<LocalInput, RemoteInput>>, Vec<LocalInput>) {
        let to_commit = {
            let n = std::cmp::max(
                std::cmp::min(
                    self.local_queue.len() as isize - self.local_delay as isize,
                    self.remote_queue.len() as isize,
                ),
                0,
            );

            std::iter::zip(
                self.local_queue.drain(..n as usize),
                self.remote_queue.drain(..n as usize),
            )
            .map(|(local, remote)| Pair { local, remote })
            .collect()
        };

        let peeked = self
            .local_queue
            .range(..std::cmp::max(self.local_queue.len() as isize - self.local_delay as isize, 0) as usize)
            .cloned()
            .collect();

        (to_commit, peeked)
    }
}
