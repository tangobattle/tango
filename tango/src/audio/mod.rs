//! Audio core: a Stream trait, a late-binding mux so the host
//! output stream can outlive any one session, and the MGBAStream
//! adapter that pulls samples out of an mgba thread and resamples
//! to the host rate.
//!
//! The audio device is strictly a CONSUMER here, never a pacing
//! dependency: sessions run their cores with sync-to-audio off (see
//! `session::new_gba_core`) and pace by wall clock instead
//! (`session::pacer`), so a host stream that stops calling
//! [`Stream::fill`] — a stalled virtual output device, a sleeping
//! headset — just leaves the core's sample ring dropping its newest
//! samples while emulation (and netplay) carries on. Rate-matching
//! against the device clock is handled by dynamic rate control in
//! [`MGBAStream::fill`] rather than by blocking the producer.

pub mod sdl;

pub const NUM_CHANNELS: usize = 2;
/// Device buffer size (frames) requested from SDL via the
/// SDL_AUDIO_DEVICE_SAMPLE_FRAMES hint (see `sdl_init`). Advisory —
/// backends can pick another quantum, and [`MGBAStream`] adapts its
/// cushion to whatever request sizes actually arrive.
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

    /// Bind a running mgba thread's audio output to this binder. Every session
    /// does this identically; a failed bind is logged (tagged with `context`)
    /// and downgraded to silence rather than aborting the session.
    pub fn bind_mgba(&self, handle: mgba::thread::Handle, context: &str) -> Option<Binding> {
        match self.bind(Some(Box::new(MGBAStream::new(handle, self.sample_rate())))) {
            Ok(b) => Some(b),
            Err(e) => {
                log::warn!("{context}: audio bind failed: {e:?}");
                None
            }
        }
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

/// Largest fractional resample-rate nudge dynamic rate control applies
/// (±0.5%): enough to cancel any realistic clock drift between the
/// emulation pace and the audio device crystal, small enough to stay
/// inaudible.
const MAX_RATE_ADJUSTMENT: f64 = 0.005;

/// Hard ceiling on the reservoir target (as a fraction of a second of
/// host-rate frames): a single absurd callback request must not pin
/// the session's audio latency sky-high forever.
const MAX_RESERVOIR_SECS: f64 = 0.25;

/// Pulls audio out of a running mgba thread, resampling from mGBA's
/// internal rate to the host audio rate.
///
/// Consumption is best-effort and self-balancing: every `fill` drains
/// whatever the core's ring holds through the resampler into a
/// host-rate reservoir, and dynamic rate control nudges the resample
/// ratio so the reservoir hovers at `reservoir_target` leftover
/// frames. The core is never throttled from here (sync-to-audio is
/// off — see `session::new_gba_core`); if callbacks stop, the core's
/// ring simply overruns and drops its newest samples, and the backlog
/// that lands here on resume is trimmed away (skip ahead) rather than
/// carried as standing latency.
///
/// The cushion is sized from what the device actually does, not a
/// constant: one observed callback quantum plus one emulated frame's
/// production burst. A device switch that grows the quantum (44.1 kHz
/// family rates ask for ~9% more host frames per callback than 48 kHz)
/// grows the target with it. After a bind or an underrun, `fill`
/// serves silence until the reservoir first reaches the target —
/// starting from full headroom instead of crackling toward it at DRC's
/// ±0.5% refill pace.
pub struct MGBAStream {
    handle: mgba::thread::Handle,
    sample_rate: u32,
    resampler: mgba::audio::AudioResampler,
    /// Scratch the resampler renders into during a fill; drained to
    /// `reservoir` before returning, so nothing persists here across
    /// fills. Sized (and lazily grown) to hold the core's entire ring
    /// converted at the current ratio — `mAudioBufferWrite` silently
    /// drops on overflow, so it must never be the limiting container.
    scratch: mgba::audio::AudioBuffer,
    /// Tracked separately because `mAudioBuffer` doesn't expose
    /// capacity through the Rust binding.
    scratch_capacity: usize,
    /// Linear staging for the scratch→reservoir copy, reused across
    /// fills.
    staging: Vec<i16>,
    /// Resampled host-rate frames waiting for the device. What a
    /// callback doesn't consume is the cushion (and the DRC input)
    /// for the next one.
    reservoir: std::collections::VecDeque<[i16; NUM_CHANNELS]>,
    /// Adaptive leftover target: max observed callback quantum plus
    /// one production burst. Running max — shrinking on a device
    /// switch would risk an underrun for no benefit beyond latency,
    /// and the stream is rebuilt per session anyway.
    reservoir_target: usize,
    /// Serve silence until the reservoir first reaches
    /// `reservoir_target`. Set at construction and again on underrun.
    priming: bool,
}

impl MGBAStream {
    pub fn new(handle: mgba::thread::Handle, sample_rate: u32) -> MGBAStream {
        // Placeholder capacity — `fill` computes the real bound from
        // the core's ring size before the first `process`.
        let scratch_capacity = 1024;
        Self {
            handle,
            sample_rate,
            resampler: mgba::audio::AudioResampler::new(),
            scratch: mgba::audio::AudioBuffer::new(scratch_capacity, NUM_CHANNELS as u32),
            scratch_capacity,
            staging: Vec::new(),
            reservoir: std::collections::VecDeque::new(),
            reservoir_target: 0,
            priming: true,
        }
    }
}

impl Stream for MGBAStream {
    fn fill(&mut self, buf: &mut [[i16; NUM_CHANNELS]]) -> usize {
        let frame_count = buf.len();

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

        // The cushion has to cover the worst gap between production
        // bursts (one emulated frame's samples land at once, ~16.7 ms
        // apart) on top of whatever the device drains per callback.
        // Two bursts, not one: pacer sleep overshoot and device-thread
        // catch-up (back-to-back callbacks after a scheduling hiccup)
        // both show up as an extra burst-sized swing in practice. Use
        // native cadence rather than fps_target — fast-forward raises
        // fps_target, and a *smaller* cushion during speed-up would be
        // exactly backwards.
        let burst = 2 * (self.sample_rate / 60) as usize;
        let cap = (self.sample_rate as f64 * MAX_RESERVOIR_SECS) as usize;
        self.reservoir_target = self.reservoir_target.max((frame_count + burst).min(cap));

        // Dynamic rate control: `faux_clock` matches consumption to the
        // fps_target-scaled production rate on paper, but the emulation
        // pacer and the device run on different clocks. Steer by the
        // observable — the leftover the last fill parked in the
        // reservoir — so the residual drift never accumulates into
        // underrun crackle or overrun drops.
        let target = self.reservoir_target as f64;
        let leftover = self.reservoir.len() as f64;
        let deviation = ((leftover - target) / target).clamp(-1.0, 1.0);
        let dest_rate = self.sample_rate as f64 * faux_clock * (1.0 - MAX_RATE_ADJUSTMENT * deviation);

        let mut core = audio_guard.core_mut();
        // Worst case `process` output: the whole ring at the current
        // ratio. Growing is safe here — scratch is empty between fills.
        let scratch_needed = (core.audio_buffer_size() as f64 * (dest_rate / core_rate)).ceil() as usize + 1;
        if scratch_needed > self.scratch_capacity {
            let new_capacity = scratch_needed.next_power_of_two();
            self.scratch = mgba::audio::AudioBuffer::new(new_capacity, NUM_CHANNELS as u32);
            self.scratch_capacity = new_capacity;
        }

        let mut core_buffer = core.audio_buffer();
        self.resampler.set_source(&mut core_buffer, core_rate, true);
        self.resampler.set_destination(&mut self.scratch, dest_rate);
        self.resampler.process();

        let produced = self.scratch.available();
        if produced > 0 {
            if self.staging.len() < produced * NUM_CHANNELS {
                self.staging.resize(produced * NUM_CHANNELS, 0);
            }
            let n = self.scratch.read(&mut self.staging[..produced * NUM_CHANNELS], produced);
            self.reservoir
                .extend(bytemuck::cast_slice::<i16, [i16; NUM_CHANNELS]>(&self.staging[..n * NUM_CHANNELS]));
        }

        // A consumption stall (device asleep, stream unserviced) parks
        // the core's whole ring here on resume. Skip ahead by dropping
        // the oldest frames — DRC would take minutes to bleed off that
        // much standing latency at ±0.5%.
        if self.reservoir.len() > self.reservoir_target * 2 {
            let excess = self.reservoir.len() - self.reservoir_target;
            self.reservoir.drain(..excess);
        }

        // Priming: hold silence until the cushion is full, so serving
        // starts with real headroom instead of underrunning on every
        // badly-phased callback while the cushion crawls up.
        if self.priming {
            if self.reservoir.len() < self.reservoir_target {
                return 0;
            }
            self.priming = false;
        }

        let served = self.reservoir.len().min(frame_count);
        for (dst, src) in buf[..served].iter_mut().zip(self.reservoir.drain(..served)) {
            *dst = src;
        }
        if served < frame_count {
            // Underrun: the cushion is gone. Re-prime — one clean gap
            // now beats seconds of repeated crackle at zero headroom.
            self.priming = true;
        }
        served
    }
}

