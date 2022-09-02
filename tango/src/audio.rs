pub mod cpal;
pub mod sdl2;

pub trait Stream {
    fn fill(&mut self, buf: &mut [[i16; NUM_CHANNELS]]) -> usize;
}

#[derive(thiserror::Error, Debug)]
pub enum BindingError {
    #[error("already bound")]
    AlreadyBound,
}

pub struct Binding {
    binder: LateBinder,
}

impl Drop for Binding {
    fn drop(&mut self) {
        *self.binder.stream.lock() = None;
    }
}

#[derive(Clone)]
pub struct LateBinder {
    sample_rate: u32,
    stream: std::sync::Arc<parking_lot::Mutex<Option<Box<dyn Stream + Send + 'static>>>>,
}

impl LateBinder {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            stream: std::sync::Arc::new(parking_lot::Mutex::new(None)),
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn bind(
        &self,
        stream: Option<Box<dyn Stream + Send + 'static>>,
    ) -> Result<Binding, BindingError> {
        let mut stream_guard = self.stream.lock();
        if stream_guard.is_some() {
            return Err(BindingError::AlreadyBound);
        }

        *stream_guard = stream;
        Ok(Binding {
            binder: self.clone(),
        })
    }
}

impl Stream for LateBinder {
    fn fill(&mut self, buf: &mut [[i16; NUM_CHANNELS]]) -> usize {
        let mut stream = self.stream.lock();
        let stream = if let Some(stream) = &mut *stream {
            stream
        } else {
            for v in buf.iter_mut() {
                *v = [0, 0];
            }
            return buf.len() / 2;
        };
        stream.fill(buf)
    }
}

pub const NUM_CHANNELS: usize = 2;

pub struct MGBAStream {
    handle: mgba::thread::Handle,
    sample_rate: u32,
}

impl MGBAStream {
    pub fn new(handle: mgba::thread::Handle, sample_rate: u32) -> MGBAStream {
        Self {
            handle,
            sample_rate,
        }
    }
}

impl Stream for MGBAStream {
    fn fill(&mut self, buf: &mut [[i16; NUM_CHANNELS]]) -> usize {
        let frame_count = buf.len();
        let linear_buf = bytemuck::cast_slice_mut(buf);

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
            let mut available = left.samples_avail() as usize;
            if available > frame_count {
                available = frame_count;
            }
            left.read_samples(linear_buf, available as i32, true);
            available
        };

        let mut right = core.audio_channel(1);
        right.set_rates(
            clock_rate as f64,
            self.sample_rate as f64 * faux_clock as f64,
        );
        right.read_samples(&mut linear_buf[1..], available as i32, true);

        available as usize
    }
}
