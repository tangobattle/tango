pub struct TimewarpStream {
    handle: mgba::thread::Handle,
    sample_rate: cpal::SampleRate,
    channels: u16,
}

unsafe impl Send for TimewarpStream {}

impl TimewarpStream {
    pub fn new(
        handle: mgba::thread::Handle,
        sample_rate: cpal::SampleRate,
        channels: u16,
    ) -> TimewarpStream {
        Self {
            handle,
            sample_rate,
            channels,
        }
    }
}

impl super::Stream for TimewarpStream {
    fn fill(&self, buf: &mut [i16]) -> usize {
        let mut audio_guard = self.handle.lock_audio();

        let mut core = audio_guard.core_mut();
        let frame_count = (buf.len() / self.channels as usize) as i32;
        let stereo = self.channels > 1;

        let clock_rate = core.as_ref().frequency();

        let mut fps_target = core.as_ref().gba().sync().unwrap().fps_target();
        if fps_target <= 0.0 {
            fps_target = 1.0;
        }
        let faux_clock = mgba::gba::audio_calculate_ratio(1.0, fps_target, 1.0);

        let available = {
            let mut left = core.audio_channel(0);
            left.set_rates(
                clock_rate as f64,
                self.sample_rate.0 as f64 * faux_clock as f64,
            );
            let mut available = left.samples_avail();
            if available > frame_count {
                available = frame_count;
            }
            left.read_samples(buf, available, stereo);
            available
        };

        if stereo {
            let mut right = core.audio_channel(1);
            right.set_rates(
                clock_rate as f64,
                self.sample_rate.0 as f64 * faux_clock as f64,
            );
            right.read_samples(&mut buf[1..], available, stereo);
        }

        available as usize * self.channels as usize
    }
}
