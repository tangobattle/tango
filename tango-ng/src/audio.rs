//! Audio core: a Stream trait, a late-binding mux so the cpal output
//! stream can outlive any one session, and the MGBAStream adapter that
//! pulls samples out of an mgba thread and resamples to the host rate.
//!
//! Ported with minor cleanups from `tango/src/audio.rs`.

pub mod cpal;

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
        *self.binder.stream.lock() = None;
    }
}

/// A `Stream` whose underlying source can be swapped at runtime. The
/// cpal output stream binds to this once at startup; sessions then bind
/// their MGBAStream into it on open and drop the Binding on close.
#[derive(Clone)]
pub struct LateBinder {
    sample_rate: u32,
    stream: std::sync::Arc<parking_lot::Mutex<Option<Box<dyn Stream + Send + 'static>>>>,
}

impl LateBinder {
    pub fn new() -> Self {
        Self {
            sample_rate: 0,
            stream: std::sync::Arc::new(parking_lot::Mutex::new(None)),
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.sample_rate = sample_rate;
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn bind(
        &self,
        stream: Option<Box<dyn Stream + Send + 'static>>,
    ) -> Result<Binding, BindingError> {
        let mut g = self.stream.lock();
        if g.is_some() {
            return Err(BindingError::AlreadyBound);
        }
        *g = stream;
        Ok(Binding { binder: self.clone() })
    }
}

impl Stream for LateBinder {
    fn fill(&mut self, buf: &mut [[i16; NUM_CHANNELS]]) -> usize {
        let mut s = self.stream.lock();
        let Some(stream) = &mut *s else {
            // Silence when nothing's bound. Returning buf.len() means
            // we consider the whole buffer "filled" so cpal doesn't
            // pad-and-loop the last samples.
            for v in buf.iter_mut() {
                *v = [0, 0];
            }
            return buf.len();
        };
        stream.fill(buf)
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
}

impl MGBAStream {
    pub fn new(handle: mgba::thread::Handle, sample_rate: u32) -> MGBAStream {
        Self {
            handle,
            sample_rate,
            resampler: mgba::audio::AudioResampler::new(),
            // 2x SAMPLES — enough to hold an above-average single
            // callback's resampler output without losing the tail,
            // small enough that the leftover doesn't accumulate into
            // perceptible A/V lag. See tango/src/audio.rs.
            dest_buffer: mgba::audio::AudioBuffer::new(SAMPLES * 2, NUM_CHANNELS as u32),
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

        let mut core = audio_guard.core_mut();
        let faux_clock = core.as_ref().calculate_framerate_ratio(fps_target as f64);
        let core_rate = core.as_ref().audio_sample_rate() as f64;
        let core_buffer_ptr = core.audio_buffer().as_mut_ptr();

        let dest_rate = self.sample_rate as f64 * faux_clock;
        let high_water =
            (frame_count as f64 + 16.0 + frame_count as f64 / 64.0) * core_rate / dest_rate;
        audio_guard.sync_mut().set_audio_high_water(high_water as u32);

        self.resampler.set_source(core_buffer_ptr, core_rate, true);
        self.resampler.set_destination(self.dest_buffer.as_mut_ptr(), dest_rate);
        self.resampler.process();

        let available = self.dest_buffer.available().min(frame_count);
        self.dest_buffer.read(&mut linear_buf[..available * NUM_CHANNELS], available);
        available
    }
}

pub trait Backend {
    fn sample_rate(&self) -> u32;
}
