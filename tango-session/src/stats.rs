//! Small rolling-window timing counter, used by the PvP session
//! status bar to compute emulator TPS. Marked once per frame
//! callback (emulator-rate) so the reading is independent of UI
//! refresh rate. Mirrors `tango::stats::Counter`.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub struct Counter {
    marks: VecDeque<Instant>,
    window_size: usize,
}

impl Counter {
    pub fn new(window_size: usize) -> Self {
        Self {
            marks: VecDeque::with_capacity(window_size),
            window_size,
        }
    }

    pub fn mark(&mut self) {
        while self.marks.len() >= self.window_size {
            self.marks.pop_front();
        }
        self.marks.push_back(Instant::now());
    }

    /// Average interval between consecutive marks. ZERO if the
    /// counter has fewer than two marks.
    pub fn mean_duration(&self) -> Duration {
        if self.marks.len() < 2 {
            return Duration::ZERO;
        }
        let mut total = Duration::ZERO;
        let mut count = 0u32;
        for (a, b) in self.marks.iter().zip(self.marks.iter().skip(1)) {
            total += *b - *a;
            count += 1;
        }
        total / count
    }
}
