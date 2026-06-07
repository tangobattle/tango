//! Audio core: a Stream trait, a late-binding mux so the host
//! output stream can outlive any one session, and the MGBAStream
//! adapter that pulls samples out of an mgba thread and resamples
//! to the host rate.

pub mod sdl;

pub const NUM_CHANNELS: usize = 2;
pub const SAMPLES: usize = 512;

pub trait Stream {
    fn fill(&mut self, buf: &mut [[i16; NUM_CHANNELS]]) -> usize;
}

#[derive(thiserror::Error, Debug)]
pub enum BindingError {
    #[error("already bound")]
    AlreadyBound,
}

/// RAII guard for an active binding — when dropped, the LateBinder is
/// reset to silence.
pub struct Binding {
    binder: LateBinder,
}

impl Drop for Binding {
    fn drop(&mut self) {
        *self.binder.stream.lock().unwrap() = None;
    }
}

/// A `Stream` whose underlying source can be swapped at runtime. The
/// host audio backend binds to this once at startup; sessions then bind
/// their MGBAStream into it on open and drop the Binding on close.
#[derive(Clone)]
pub struct LateBinder {
    sample_rate: u32,
    stream: std::sync::Arc<std::sync::Mutex<Option<Box<dyn Stream + Send + 'static>>>>,
    /// User-facing master volume, stored as raw f32 bits in an atomic
    /// so the UI thread can mutate it while the audio thread reads it
    /// on each `fill`. Domain is [0.0, 1.0]; values outside clamp.
    volume: std::sync::Arc<std::sync::atomic::AtomicU32>,
}

impl LateBinder {
    pub fn new() -> Self {
        Self {
            sample_rate: 0,
            stream: std::sync::Arc::new(std::sync::Mutex::new(None)),
            volume: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(1.0_f32.to_bits())),
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.sample_rate = sample_rate;
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Set the master output volume. Clamped to `[0.0, 1.0]`. Cheap
    /// (single atomic store) — safe to call from the UI thread.
    pub fn set_volume(&self, v: f32) {
        let v = v.clamp(0.0, 1.0);
        self.volume.store(v.to_bits(), std::sync::atomic::Ordering::Relaxed);
    }

    fn read_volume(&self) -> f32 {
        f32::from_bits(self.volume.load(std::sync::atomic::Ordering::Relaxed))
    }

    pub fn bind(&self, stream: Option<Box<dyn Stream + Send + 'static>>) -> Result<Binding, BindingError> {
        let mut g = self.stream.lock().unwrap();
        if g.is_some() {
            return Err(BindingError::AlreadyBound);
        }
        *g = stream;
        Ok(Binding { binder: self.clone() })
    }
}

impl Stream for LateBinder {
    fn fill(&mut self, buf: &mut [[i16; NUM_CHANNELS]]) -> usize {
        let mut s = self.stream.lock().unwrap();

        let Some(stream) = &mut *s else {
            for v in buf.iter_mut() {
                *v = [0, 0];
            }
            return buf.len();
        };

        let n = stream.fill(buf);

        // Master volume gain. Skip the multiply at unity so the
        // common case is free.
        let v = self.read_volume();
        if v < 1.0 {
            for sample in &mut buf[..n] {
                for ch in sample.iter_mut() {
                    *ch = (*ch as f32 * v) as i16;
                }
            }
        }
        n
    }
}

/// Pulls audio out of a running mgba thread, resampling from mGBA's
/// internal rate to the host audio rate. The high-water adjustment
/// follows the same formula as mGBA's SDL frontend so high-SOUNDBIAS
/// games (Battle Network 4+) don't starve.
pub struct MGBAStream {
    handle: mgba::thread::Handle,
    sample_rate: u32,
    resampler: mgba::audio::AudioResampler,
    dest_buffer: mgba::audio::AudioBuffer,
    /// Tracked separately because `mAudioBuffer` doesn't expose
    /// capacity through the Rust binding; grown lazily in `fill`.
    dest_capacity: usize,
}

impl MGBAStream {
    pub fn new(handle: mgba::thread::Handle, sample_rate: u32) -> MGBAStream {
        let dest_capacity = SAMPLES * 2;
        Self {
            handle,
            sample_rate,
            resampler: mgba::audio::AudioResampler::new(),
            dest_buffer: mgba::audio::AudioBuffer::new(dest_capacity, NUM_CHANNELS as u32),
            dest_capacity,
        }
    }
}

impl Stream for MGBAStream {
    fn fill(&mut self, buf: &mut [[i16; NUM_CHANNELS]]) -> usize {
        let frame_count = buf.len();
        let linear_buf: &mut [i16] = bytemuck::cast_slice_mut(buf);

        let mut audio_guard = self.handle.lock_audio();
        let mut fps_target = audio_guard.sync().fps_target();
        if fps_target <= 0.0 {
            fps_target = 1.0;
        }

        let (core_rate, faux_clock) = {
            let core = audio_guard.core_mut();
            (
                core.as_ref().audio_sample_rate() as f64,
                core.as_ref().calculate_framerate_ratio(fps_target as f64),
            )
        };

        let dest_rate = self.sample_rate as f64 * faux_clock;
        let high_water = (frame_count as f64 + 16.0 + frame_count as f64 / 64.0) * core_rate / dest_rate;
        audio_guard.sync_mut().set_audio_high_water(high_water as u32);

        let needed = frame_count.saturating_mul(2);
        if needed > self.dest_capacity {
            let new_capacity = needed.next_power_of_two().max(SAMPLES * 2);
            self.dest_buffer = mgba::audio::AudioBuffer::new(new_capacity, NUM_CHANNELS as u32);
            self.dest_capacity = new_capacity;
        }

        let mut core = audio_guard.core_mut();
        let mut core_buffer = core.audio_buffer();
        self.resampler.set_source(&mut core_buffer, core_rate, true);
        self.resampler.set_destination(&mut self.dest_buffer, dest_rate);
        self.resampler.process();

        let available = self.dest_buffer.available().min(frame_count);
        self.dest_buffer
            .read(&mut linear_buf[..available * NUM_CHANNELS], available);
        available
    }
}

pub trait Backend {
    fn sample_rate(&self) -> u32;
}
