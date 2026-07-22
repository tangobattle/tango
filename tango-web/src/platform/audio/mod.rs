//! Audio core: a Stream trait, a late-binding mux so the host output
//! stream can outlive any one session, and the LinkStream adapter for
//! live links. `web.rs` is the sink: an AudioWorklet fed by the runtime
//! pump, whose ring buffer is tango-web-worklet's wasm module.
//! (`replay.rs` is present but not compiled until the replay playback
//! port lands.)

#[cfg(not(target_arch = "wasm32"))]
pub mod sdl;
#[cfg(target_arch = "wasm32")]
pub mod web;

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
/// their LinkStream into it on open and drop the Binding on close.
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

    #[allow(dead_code)]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Set the master output volume. Clamped to `[0.0, 1.0]`. Cheap
    /// (single atomic store) — safe to call from the UI thread.
    pub fn set_volume(&self, v: f32) {
        let v = v.clamp(0.0, 1.0);
        self.volume
            .store(v.to_bits(), std::sync::atomic::Ordering::Relaxed);
    }

    fn read_volume(&self) -> f32 {
        f32::from_bits(self.volume.load(std::sync::atomic::Ordering::Relaxed))
    }

    pub fn bind(
        &self,
        stream: Option<Box<dyn Stream + Send + 'static>>,
    ) -> Result<Binding, BindingError> {
        let mut g = self.stream.lock().unwrap();
        if g.is_some() {
            return Err(BindingError::AlreadyBound);
        }
        *g = stream;
        Ok(Binding {
            binder: self.clone(),
        })
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

/// Seconds of queued source audio the servo holds the core's buffer at.
const TARGET_QUEUED_SECS: f64 = 0.05;
/// Max resample-ratio trim the servo applies (±0.5%) — inaudible, but
/// enough to converge the queue.
const MAX_TRIM: f64 = 0.005;
/// Queue depth (in targets) past which we discard oldest samples in one
/// go instead of trimming. Healthy operation never exceeds ~1.7x, and
/// rollbacks can't get here (re-simulation replaces its revoked audio
/// exactly) — only producer bursts do, e.g. the sim catching up after
/// the consumer stalled. One skip beats seconds of extra latency.
const DISCARD_FACTOR: f64 = 3.0;

/// Pulls audio out of the presented core of a live link, resampling
/// from mGBA's internal rate to the host rate. The simulation is paced
/// by its own match clock (not by this stream), so buffer regulation
/// happens on the consumption side: a servo trims the claimed source
/// rate so the core's queue converges on a fixed target, and the
/// destination rate follows the published fps target (the faux clock),
/// so a throttled simulation stretches playback instead of starving it.
pub struct LinkStream {
    access: crate::session::LinkAccess,
    shared: std::sync::Arc<crate::session::SharedSession>,
    sample_rate: u32,
    resampler: mgba::audio::AudioResampler,
    dest_buffer: mgba::audio::OwnedAudioBuffer,
    /// Tracked separately because `mAudioBuffer` doesn't expose
    /// capacity through the Rust binding; grown lazily in `fill`.
    dest_capacity: usize,
    /// Scratch for bulk-discarding backlog.
    discard: Vec<i16>,
    /// Serve silence until the queue reaches target once. Latched at
    /// construction and on any fill the queue couldn't cover — the
    /// stall signature (a hidden tab throttling the sim, a pause, a
    /// link swap). Without it the post-stall rebuild happens at the
    /// servo's ~5 ms of queue per second and the stream rides
    /// near-empty for ~10 seconds, crackling on every jitter trough.
    priming: bool,
}

impl LinkStream {
    pub fn new(
        access: crate::session::LinkAccess,
        shared: std::sync::Arc<crate::session::SharedSession>,
        sample_rate: u32,
    ) -> LinkStream {
        let dest_capacity = SAMPLES * 2;
        Self {
            access,
            shared,
            sample_rate,
            resampler: mgba::audio::AudioResampler::new(),
            dest_buffer: mgba::audio::OwnedAudioBuffer::new(dest_capacity, NUM_CHANNELS as u32),
            dest_capacity,
            discard: Vec::new(),
            priming: true,
        }
    }
}

impl Stream for LinkStream {
    fn fill(&mut self, buf: &mut [[i16; NUM_CHANNELS]]) -> usize {
        let frame_count = buf.len();
        let linear_buf: &mut [i16] = bytemuck::cast_slice_mut(buf);

        let fps_target = f32::from_bits(
            self.shared
                .fps_target
                .load(std::sync::atomic::Ordering::Relaxed),
        );
        if fps_target <= 0.0 {
            // Paused / ended — silence, and a stall by definition.
            self.priming = true;
            return 0;
        }
        let player = self
            .shared
            .view_player
            .load(std::sync::atomic::Ordering::Relaxed);

        let needed = frame_count.saturating_mul(2);
        if needed > self.dest_capacity {
            let new_capacity = needed.next_power_of_two().max(SAMPLES * 2);
            self.dest_buffer = mgba::audio::OwnedAudioBuffer::new(new_capacity, NUM_CHANNELS as u32);
            self.dest_capacity = new_capacity;
        }

        let (resampler, dest_buffer, discard, priming) = (
            &mut self.resampler,
            &mut self.dest_buffer,
            &mut self.discard,
            &mut self.priming,
        );
        let out_rate = self.sample_rate as f64;
        let pulled = self.access.with_link(|link| {
            let player = player.min(link.num_players() - 1);
            // Production rate follows SOUNDBIAS and can change at runtime.
            let rate = link.core(player).audio_sample_rate() as f64;
            // Faux clock: production scales with the sim's pace, so a
            // throttled sim stretches playback rather than starving.
            let faux_clock = link
                .core(player)
                .calculate_framerate_ratio(fps_target as f64);
            let core = link.core_mut(player);
            let mut source = core.audio_buffer();

            let target = rate * TARGET_QUEUED_SECS;
            let queued = source.available() as f64;
            if queued > target * DISCARD_FACTOR {
                // Deep backlog (rollback re-sim burst): skip oldest
                // samples in one go rather than pitch-warping through.
                let n = (queued - target) as usize;
                discard.resize(n * NUM_CHANNELS, 0);
                source.read(discard, n);
            }

            // Re-prime across stalls: hold silence until the queue is
            // back at target, instead of riding near-empty at the
            // servo's slow refill. (The latch is set on short delivery
            // below — the queue never reads exactly zero, the resampler
            // leaves fractional residue when it runs dry.)
            let queued = source.available() as f64;
            if *priming {
                if queued < target {
                    return;
                }
                *priming = false;
            }

            // Servo: nudge the claimed source rate so the queue
            // converges on the target.
            let trim = MAX_TRIM * ((queued - target) / target).clamp(-1.0, 1.0);

            resampler.set_source(&mut source, rate * (1.0 + trim), true);
            resampler.set_destination(dest_buffer, out_rate * faux_clock);
            resampler.process();
        });
        if pulled.is_none() {
            // Link unavailable (booting / seek chase): silence, and a
            // stall by definition.
            self.priming = true;
            return 0;
        }

        // A fill the queue couldn't cover in full is the stall
        // signature — and the moment an artifact was unavoidable
        // anyway. Latch priming so recovery is one clean gap instead
        // of seconds of jitter-trough crackle at a near-empty queue.
        if self.dest_buffer.available() < frame_count {
            self.priming = true;
        }

        let available = self.dest_buffer.available().min(frame_count);
        self.dest_buffer
            .read(&mut linear_buf[..available * NUM_CHANNELS], available);
        available
    }
}

