//! Self-healing CPAL default-device audio output.
//!
//! Sessions always produce stereo i16 at [`audio::MIX_SAMPLE_RATE`].
//! The callback converts that stable mix to the default device's
//! negotiated sample rate, channel count, and PCM sample format. This
//! keeps a live session valid when the OS moves the default route to a
//! device with a different native format.
//!
//! [`Backend`] owns a small supervisor thread as well as the native
//! stream. The supervisor re-queries the default route and format,
//! consumes CPAL's stream-error signal, watches callback liveness, and
//! retries failed opens. Device recovery therefore does not depend on
//! the UI event loop or redraw cadence.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use anyhow::Context;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SupportedBufferSize};

use crate::platform::audio;

const WATCH_INTERVAL: Duration = Duration::from_millis(500);
const STALL_TIMEOUT: Duration = Duration::from_secs(2);
const RETRY_DELAY: Duration = Duration::from_secs(5);
const OPEN_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ConfigSnapshot {
    channels: u16,
    sample_rate: u32,
    sample_format: SampleFormat,
}

/// Current default output route. Device IDs are stable where the host
/// API provides one; the name is a fallback for an endpoint whose ID
/// cannot temporarily be queried during a topology transition.
#[derive(Clone, Debug, Eq, PartialEq)]
struct DeviceSnapshot {
    id: Option<cpal::DeviceId>,
    name: Option<String>,
    config: Option<ConfigSnapshot>,
}

impl DeviceSnapshot {
    /// Re-query the default route rather than retaining a `Device`:
    /// CPAL device handles may become invalid after disconnect.
    fn capture() -> Self {
        let host = cpal::default_host();
        let Some(device) = host.default_output_device() else {
            return Self {
                id: None,
                name: None,
                config: None,
            };
        };

        let config = device.default_output_config().ok().map(|config| ConfigSnapshot {
            channels: config.channels(),
            sample_rate: config.sample_rate(),
            sample_format: config.sample_format(),
        });

        Self {
            id: device.id().ok(),
            name: Some(device.to_string()),
            config,
        }
    }
}

struct OutputState<S> {
    source: S,
    source_buf: Vec<[i16; audio::NUM_CHANNELS]>,
    source_index: usize,
    current: [f32; audio::NUM_CHANNELS],
    next: [f32; audio::NUM_CHANNELS],
    phase: f64,
    source_step: f64,
    initialized: bool,
    output_channels: usize,
}

impl<S: audio::Stream> OutputState<S> {
    fn new(source: S, output_sample_rate: u32, output_channels: usize) -> Self {
        Self {
            source,
            source_buf: Vec::new(),
            source_index: 0,
            current: [0.0; audio::NUM_CHANNELS],
            next: [0.0; audio::NUM_CHANNELS],
            phase: 0.0,
            source_step: audio::MIX_SAMPLE_RATE as f64 / output_sample_rate as f64,
            initialized: false,
            output_channels,
        }
    }

    fn pull_source_frame(&mut self) -> [f32; audio::NUM_CHANNELS] {
        if self.source_index >= self.source_buf.len() {
            self.source_buf.resize(audio::SAMPLES, [0; audio::NUM_CHANNELS]);
            let filled = self.source.fill(&mut self.source_buf).min(self.source_buf.len());
            self.source_buf[filled..].fill([0; audio::NUM_CHANNELS]);
            self.source_index = 0;
        }

        let frame = self.source_buf[self.source_index];
        self.source_index += 1;
        [frame[0] as f32 / 32768.0, frame[1] as f32 / 32768.0]
    }

    /// Linear conversion is intentionally stateful across callbacks:
    /// callback quantum changes do not introduce a phase discontinuity.
    fn next_output_frame(&mut self) -> [f32; audio::NUM_CHANNELS] {
        if !self.initialized {
            self.current = self.pull_source_frame();
            self.next = self.pull_source_frame();
            self.initialized = true;
        }

        let phase = self.phase as f32;
        let frame = [
            self.current[0] + (self.next[0] - self.current[0]) * phase,
            self.current[1] + (self.next[1] - self.current[1]) * phase,
        ];

        self.phase += self.source_step;
        while self.phase >= 1.0 {
            self.current = self.next;
            self.next = self.pull_source_frame();
            self.phase -= 1.0;
        }

        frame
    }

    fn render<T>(&mut self, output: &mut [T])
    where
        T: Sample + FromSample<f32>,
    {
        if self.output_channels == 0 {
            output.fill(T::EQUILIBRIUM);
            return;
        }

        let mut frames = output.chunks_exact_mut(self.output_channels);
        for frame in &mut frames {
            let [left, right] = self.next_output_frame();
            frame.fill(T::EQUILIBRIUM);
            if self.output_channels == 1 {
                frame[0] = T::from_sample((left + right) * 0.5);
            } else {
                // CPAL does not expose a channel map. Host APIs put
                // front-left/front-right first for ordinary PCM
                // layouts, so stereo occupies those and any surround
                // channels remain silent.
                frame[0] = T::from_sample(left);
                frame[1] = T::from_sample(right);
            }
        }
        frames.into_remainder().fill(T::EQUILIBRIUM);
    }

    fn render_data(&mut self, output: &mut cpal::Data) {
        match output.sample_format() {
            SampleFormat::I8 => self.render(output.as_slice_mut::<i8>().unwrap()),
            SampleFormat::I16 => self.render(output.as_slice_mut::<i16>().unwrap()),
            SampleFormat::I24 => self.render(output.as_slice_mut::<cpal::I24>().unwrap()),
            SampleFormat::I32 => self.render(output.as_slice_mut::<i32>().unwrap()),
            SampleFormat::I64 => self.render(output.as_slice_mut::<i64>().unwrap()),
            SampleFormat::U8 => self.render(output.as_slice_mut::<u8>().unwrap()),
            SampleFormat::U16 => self.render(output.as_slice_mut::<u16>().unwrap()),
            SampleFormat::U24 => self.render(output.as_slice_mut::<cpal::U24>().unwrap()),
            SampleFormat::U32 => self.render(output.as_slice_mut::<u32>().unwrap()),
            SampleFormat::U64 => self.render(output.as_slice_mut::<u64>().unwrap()),
            SampleFormat::F32 => self.render(output.as_slice_mut::<f32>().unwrap()),
            SampleFormat::F64 => self.render(output.as_slice_mut::<f64>().unwrap()),
            // DSD is not accepted by `open_stream`; reaching this
            // would mean the host violated the stream contract.
            _ => unreachable!("CPAL callback changed its negotiated sample format"),
        }
    }
}

struct LiveStream {
    /// Held to keep the native stream and callback alive.
    _stream: cpal::Stream,
    faulted: Arc<AtomicBool>,
    heartbeat: Arc<AtomicU64>,
}

fn open_stream(source: audio::LateBinder) -> anyhow::Result<LiveStream> {
    let host = cpal::default_host();
    let device = host.default_output_device().context("no default output device")?;
    let supported = device.default_output_config().context("query default output config")?;
    let sample_format = supported.sample_format();
    if !is_pcm(sample_format) {
        anyhow::bail!("unsupported default output sample format: {sample_format}");
    }
    if supported.channels() == 0 || supported.sample_rate() == 0 {
        anyhow::bail!(
            "invalid default output config: {} Hz / {} channels",
            supported.sample_rate(),
            supported.channels()
        );
    }

    let mut config = supported.config();
    if let SupportedBufferSize::Range { min, max } = *supported.buffer_size() {
        config.buffer_size = cpal::BufferSize::Fixed((audio::SAMPLES as u32).clamp(min, max));
    }

    let faulted = Arc::new(AtomicBool::new(false));
    let heartbeat = Arc::new(AtomicU64::new(0));
    let callback_heartbeat = heartbeat.clone();
    let callback_faulted = faulted.clone();
    let mut output = OutputState::new(source, config.sample_rate, config.channels as usize);
    let stream = device
        .build_output_stream_raw(
            config,
            sample_format,
            move |data, _| {
                output.render_data(data);
                callback_heartbeat.fetch_add(1, Ordering::Relaxed);
            },
            move |error| {
                use cpal::ErrorKind;
                match error.kind() {
                    // These are advisory; the stream remains usable.
                    ErrorKind::Xrun => log::warn!("cpal audio underrun/overrun: {error}"),
                    ErrorKind::RealtimeDenied => {
                        log::warn!("cpal audio real-time scheduling unavailable: {error}")
                    }
                    // A route change may preserve the stream, but
                    // rebuilding lets us renegotiate a new native
                    // sample rate/channel layout.
                    ErrorKind::DeviceChanged => {
                        log::info!("cpal audio route changed: {error}");
                        callback_faulted.store(true, Ordering::Release);
                    }
                    _ => {
                        log::error!("cpal audio stream failed: {error}");
                        callback_faulted.store(true, Ordering::Release);
                    }
                }
            },
            // Avoid pinning the supervisor (and application shutdown)
            // forever in a wedged host API. Backends that cannot
            // enforce initialization timeouts document this as
            // advisory and may ignore it.
            Some(OPEN_TIMEOUT),
        )
        .context("build default output stream")?;
    stream.play().context("start default output stream")?;

    let id = device
        .id()
        .map(|id| id.to_string())
        .unwrap_or_else(|_| "<unknown id>".to_owned());
    log::info!(
        "cpal audio: {device} ({id}), {} Hz / {}ch / {sample_format}",
        config.sample_rate,
        config.channels
    );

    Ok(LiveStream {
        _stream: stream,
        faulted,
        heartbeat,
    })
}

#[derive(Default)]
struct StopSignal {
    stopped: Mutex<bool>,
    wake: Condvar,
}

pub struct Backend {
    stop: Arc<StopSignal>,
    supervisor: Option<std::thread::JoinHandle<()>>,
}

impl Backend {
    /// Start the recovery supervisor. Absence of an output device is
    /// not a construction error: the supervisor stays alive and opens
    /// one when it appears.
    pub fn new(source: audio::LateBinder) -> anyhow::Result<Self> {
        let stop = Arc::new(StopSignal::default());
        let thread_stop = stop.clone();
        let supervisor = std::thread::Builder::new()
            .name("audio-supervisor".to_owned())
            .spawn(move || supervise(source, thread_stop))
            .context("spawn audio supervisor")?;
        Ok(Self {
            stop,
            supervisor: Some(supervisor),
        })
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        *self.stop.stopped.lock().unwrap() = true;
        self.stop.wake.notify_one();
        if let Some(supervisor) = self.supervisor.take() {
            if supervisor.join().is_err() {
                log::error!("audio supervisor panicked");
            }
        }
    }
}

fn supervise(source: audio::LateBinder, stop: Arc<StopSignal>) {
    let mut snapshot = DeviceSnapshot::capture();
    let mut live: Option<LiveStream> = None;
    let mut next_retry = Instant::now();
    let mut last_heartbeat = 0;
    let mut last_callback = Instant::now();

    loop {
        let now = Instant::now();
        let new_snapshot = DeviceSnapshot::capture();
        let route_changed = new_snapshot != snapshot;
        if route_changed {
            log::info!(
                "audio: default route/config changed: {:?} -> {:?}",
                snapshot,
                new_snapshot
            );
            snapshot = new_snapshot;
        }

        let mut rebuild_reason = route_changed.then_some("default route/config changed");
        if let Some(stream) = &live {
            let heartbeat = stream.heartbeat.load(Ordering::Relaxed);
            if heartbeat != last_heartbeat {
                last_heartbeat = heartbeat;
                last_callback = now;
            } else if now.duration_since(last_callback) >= STALL_TIMEOUT {
                rebuild_reason = Some("output callback stalled");
            }
            if stream.faulted.load(Ordering::Acquire) {
                rebuild_reason = Some("stream error");
            }
        }

        if let Some(reason) = rebuild_reason {
            if live.is_some() {
                log::warn!("audio: {reason}, rebuilding default output stream");
                // Release a stale/exclusive endpoint before opening
                // its replacement (important for direct ALSA devices).
                live = None;
            }
            next_retry = now;
        }

        if live.is_none() && now >= next_retry {
            match open_stream(source.clone()) {
                Ok(stream) => {
                    last_heartbeat = stream.heartbeat.load(Ordering::Relaxed);
                    last_callback = Instant::now();
                    live = Some(stream);
                }
                Err(error) => {
                    log::warn!("audio: open failed, retrying in 5 seconds: {error:?}");
                    next_retry = now + RETRY_DELAY;
                }
            }
        }

        let stopped = stop.stopped.lock().unwrap();
        if *stopped {
            break;
        }
        let (stopped, _) = stop.wake.wait_timeout(stopped, WATCH_INTERVAL).unwrap();
        if *stopped {
            break;
        }
    }
}

fn is_pcm(format: SampleFormat) -> bool {
    matches!(
        format,
        SampleFormat::I8
            | SampleFormat::I16
            | SampleFormat::I24
            | SampleFormat::I32
            | SampleFormat::I64
            | SampleFormat::U8
            | SampleFormat::U16
            | SampleFormat::U24
            | SampleFormat::U32
            | SampleFormat::U64
            | SampleFormat::F32
            | SampleFormat::F64
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Ramp {
        next: i16,
    }

    impl audio::Stream for Ramp {
        fn fill(&mut self, buf: &mut [[i16; audio::NUM_CHANNELS]]) -> usize {
            for frame in buf.iter_mut() {
                *frame = [self.next, -self.next];
                self.next = self.next.saturating_add(1_000);
            }
            buf.len()
        }
    }

    #[test]
    fn resampling_phase_is_continuous_across_callbacks() {
        let mut state = OutputState::new(Ramp { next: 0 }, 96_000, 2);
        let mut first = [0.0_f32; 6];
        let mut second = [0.0_f32; 4];
        state.render(&mut first);
        state.render(&mut second);

        let scale = 1.0 / 32768.0;
        let left: Vec<_> = first
            .chunks_exact(2)
            .chain(second.chunks_exact(2))
            .map(|frame| frame[0])
            .collect();
        let expected = [0.0, 500.0, 1_000.0, 1_500.0, 2_000.0].map(|sample| sample * scale);
        for (actual, expected) in left.iter().zip(expected) {
            assert!((actual - expected).abs() < 1.0e-6);
        }
    }

    #[test]
    fn mono_output_downmixes_stereo() {
        struct Stereo;
        impl audio::Stream for Stereo {
            fn fill(&mut self, buf: &mut [[i16; audio::NUM_CHANNELS]]) -> usize {
                buf.fill([16_384, 0]);
                buf.len()
            }
        }

        let mut state = OutputState::new(Stereo, audio::MIX_SAMPLE_RATE, 1);
        let mut output = [0.0_f32; 2];
        state.render(&mut output);
        assert_eq!(output, [0.25, 0.25]);
    }
}
