pub struct Counter {
    marks: std::collections::VecDeque<std::time::Instant>,
    window_size: usize,
}

impl Counter {
    pub fn new(window_size: usize) -> Self {
        Self {
            marks: std::collections::VecDeque::with_capacity(window_size),
            window_size,
        }
    }

    pub fn mark(&mut self) {
        while self.marks.len() >= self.window_size {
            self.marks.pop_front();
        }
        self.marks.push_back(std::time::Instant::now());
    }

    pub fn mean_duration(&self) -> std::time::Duration {
        let durations = self
            .marks
            .iter()
            .zip(self.marks.iter().skip(1))
            .map(|(x, y)| *y - *x)
            .collect::<Vec<std::time::Duration>>();
        if durations.is_empty() {
            return std::time::Duration::ZERO;
        }
        durations.iter().sum::<std::time::Duration>() / durations.len() as u32
    }
}

pub struct DeltaCounter {
    marks: std::collections::VecDeque<std::time::Duration>,
    window_size: usize,
}

impl DeltaCounter {
    pub fn new(window_size: usize) -> Self {
        Self {
            marks: std::collections::VecDeque::with_capacity(window_size),
            window_size,
        }
    }

    pub fn mark(&mut self, d: std::time::Duration) {
        while self.marks.len() >= self.window_size {
            self.marks.pop_front();
        }
        self.marks.push_back(d);
    }

    #[allow(dead_code)]
    pub fn mean(&self) -> std::time::Duration {
        self.marks.iter().sum::<std::time::Duration>() / self.marks.len() as u32
    }

    pub fn median(&self) -> std::time::Duration {
        if self.marks.is_empty() {
            return std::time::Duration::ZERO;
        }

        let mut marks = self.marks.iter().collect::<Vec<_>>();
        let (_, v, _) = marks.select_nth_unstable(self.marks.len() / 2);
        **v
    }
}
