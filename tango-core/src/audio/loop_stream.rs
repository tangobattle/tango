pub struct LoopStream {
    samples: Vec<i16>,
    loop_start: usize,
    offset: usize,
}

impl LoopStream {
    pub fn new(samples: Vec<i16>, loop_start: usize) -> Self {
        Self {
            samples,
            loop_start,
            offset: 0,
        }
    }
}

const NUM_CHANNELS: usize = 2;

impl super::Stream for LoopStream {
    fn fill(&mut self, buf: &mut [i16]) -> usize {
        let mut n = buf.len() / NUM_CHANNELS;
        while n > 0 {
            let m = std::cmp::min(n, self.samples.len() / NUM_CHANNELS - self.offset);
            buf.copy_from_slice(
                &self.samples[self.offset * NUM_CHANNELS..(self.offset + m) * NUM_CHANNELS],
            );
            n -= m;
            self.offset += m;
            if self.offset >= self.samples.len() / NUM_CHANNELS {
                self.offset = self.loop_start;
            }
        }
        buf.len()
    }
}
