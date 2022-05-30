const NUM_CHANNELS: usize = 2;

pub struct MGBAStretchStream {
    handle: mgba::thread::Handle,
    sample_rate: i32,
}

impl MGBAStretchStream {
    pub fn new(handle: mgba::thread::Handle, sample_rate: i32) -> MGBAStretchStream {
        Self {
            handle,
            sample_rate,
        }
    }
}

impl sdl2::audio::AudioCallback for MGBAStretchStream {
    type Channel = i16;

    fn callback(&mut self, buf: &mut [i16]) {
        let frame_count = (buf.len() / NUM_CHANNELS) as i32;

        let mut audio_guard = self.handle.lock_audio();

        let mut fps_target = audio_guard.sync().fps_target();
        if fps_target <= 0.0 {
            fps_target = 1.0;
        }
        let faux_clock = mgba::gba::audio_calculate_ratio(1.0, fps_target, 1.0);

        let mut core = audio_guard.core_mut();

        let clock_rate = core.as_ref().frequency();

        let available = {
            let mut left = core.audio_channel(0);
            left.set_rates(
                clock_rate as f64,
                self.sample_rate as f64 * faux_clock as f64,
            );
            let mut available = left.samples_avail();
            if available > frame_count {
                available = frame_count;
            }
            left.read_samples(buf, available, true);
            available
        };

        let mut right = core.audio_channel(1);
        right.set_rates(
            clock_rate as f64,
            self.sample_rate as f64 * faux_clock as f64,
        );
        right.read_samples(&mut buf[1..], available, true);

        for i in &mut buf[available as usize * NUM_CHANNELS..] {
            *i = 0;
        }
    }
}
