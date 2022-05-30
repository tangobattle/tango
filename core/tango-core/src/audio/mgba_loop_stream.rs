const NUM_CHANNELS: usize = 2;

pub struct MGBALoopStream {
    handle: mgba::thread::Handle,
    buf: Vec<i16>,
    sample_rate: i32,
}

impl MGBALoopStream {
    pub fn new(handle: mgba::thread::Handle, sample_rate: i32) -> MGBALoopStream {
        Self {
            handle,
            buf: vec![],
            sample_rate,
        }
    }
}

impl sdl2::audio::AudioCallback for MGBALoopStream {
    type Channel = i16;

    fn callback(&mut self, buf: &mut [i16]) {
        let buf_frame_count = (buf.len() / NUM_CHANNELS) as i32;

        let mut audio_guard = self.handle.lock_audio();

        let mut fps_target = audio_guard.sync().fps_target();
        if fps_target <= 0.0 {
            fps_target = 1.0;
        }

        let faux_clock = mgba::gba::audio_calculate_ratio(1.0, fps_target, 1.0);
        let frame_count = (buf_frame_count as f32 / faux_clock) as i32;
        if self.buf.len() < frame_count as usize * NUM_CHANNELS {
            self.buf.resize(frame_count as usize * NUM_CHANNELS, 0);
        }

        let mut core = audio_guard.core_mut();

        let clock_rate = core.as_ref().frequency();

        let available = {
            let mut left = core.audio_channel(0);
            left.set_rates(clock_rate as f64, self.sample_rate as f64);
            let mut available = left.samples_avail();
            if available > frame_count {
                available = frame_count;
            }
            left.read_samples(&mut self.buf, available, true);
            available
        };

        let mut right = core.audio_channel(1);
        right.set_rates(clock_rate as f64, self.sample_rate as f64);
        right.read_samples(&mut self.buf[1..], available, true);

        for chunk in &mut buf.chunks_mut(available as usize * 2) {
            chunk.copy_from_slice(&self.buf[..chunk.len()]);
        }
    }
}
