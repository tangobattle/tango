//! sdl3-backed audio output (native), lifted from the desktop client.
//! Opens a default playback stream at 48 kHz / stereo / i16 — the
//! shape [`super::Stream`] already produces — and pulls whatever
//! `Stream` you hand it (the runtime's `LateBinder`) from SDL's audio
//! thread. Unlike the web backend there is no push pump: the callback
//! IS the consumer, so the runtime pump has nothing to do for audio.
//!
//! SDL itself is initialized in [`crate::platform::sdl_init`]; this
//! module just borrows the global `Sdl` to grab an `AudioSubsystem`
//! and open the stream. The resulting backend must stay on the main
//! thread — `AudioStreamWithCallback` is `!Send` because it holds an
//! `AudioSubsystem` clone.
//!
//! The output device can die under us (a USB DAC unplugged, a virtual
//! endpoint dropping). SDL migrates a default-device stream across a
//! default *change*, but can't resurrect a stream whose endpoint went
//! away — so the runtime's housekeeping diffs [`playback_device_ids`]
//! once a second and rebuilds the [`Backend`] when the topology moves.

use sdl3::audio::{AudioCallback, AudioFormat, AudioSpec, AudioStream, AudioStreamWithCallback};

use crate::platform::audio;
use crate::platform::sdl_init;

/// Current playback-device topology, sorted for order-insensitive
/// comparison. Polling the list rather than watching
/// `AudioDeviceAdded`/`Removed` events is deliberate: SDL parks those
/// events in its pending list until the event pump runs, but the
/// device *list* is kept current by SDL's notification thread.
pub fn playback_device_ids(audio: &sdl3::AudioSubsystem) -> Vec<sdl3::audio::AudioDeviceID> {
    let mut ids = audio.audio_playback_device_ids().unwrap_or_default();
    ids.sort_by_key(|id| id.id().0);
    ids
}

const TARGET_SAMPLE_RATE: i32 = 48000;
const TARGET_CHANNELS: i32 = audio::NUM_CHANNELS as i32;

struct CallbackImpl {
    stream: Box<dyn audio::Stream + Send + 'static>,
    /// Scratch reused across SDL callback invocations. SDL's
    /// `requested` size can vary call to call (the buffer hint is
    /// advisory), so we grow lazily.
    buf: Vec<[i16; audio::NUM_CHANNELS]>,
}

impl AudioCallback<i16> for CallbackImpl {
    fn callback(&mut self, stream: &mut AudioStream, requested: i32) {
        let requested = requested.max(0) as usize;
        let frames = requested / audio::NUM_CHANNELS;
        if frames == 0 {
            return;
        }
        if self.buf.len() < frames {
            self.buf.resize(frames, [0, 0]);
        }
        let filled = self.stream.fill(&mut self.buf[..frames]);
        // Pad with silence if the source underran — `put_data_i16`
        // takes whatever we give it, and the unfilled tail would
        // otherwise be stale samples from a prior callback.
        for v in &mut self.buf[filled..frames] {
            *v = [0, 0];
        }
        let linear: &[i16] = bytemuck::cast_slice(&self.buf[..frames]);
        if let Err(e) = stream.put_data_i16(linear) {
            log::error!("sdl audio put_data: {e}");
        }
    }
}

pub struct Backend {
    /// Held to keep the stream + device alive — drop tears down
    /// the SDL audio thread.
    _stream: AudioStreamWithCallback<CallbackImpl>,
    sample_rate: u32,
}

impl Backend {
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn new(stream: impl audio::Stream + Send + 'static) -> anyhow::Result<Self> {
        let spec = AudioSpec {
            freq: Some(TARGET_SAMPLE_RATE),
            channels: Some(TARGET_CHANNELS),
            format: Some(AudioFormat::s16_sys()),
        };
        let callback = CallbackImpl {
            stream: Box::new(stream),
            buf: Vec::new(),
        };
        let sdl = sdl_init::sdl().ok_or_else(|| anyhow::anyhow!("sdl not initialized"))?;
        let audio = sdl.audio().map_err(|e| anyhow::anyhow!("sdl audio subsystem: {e}"))?;
        let stream_with_cb = audio
            .open_playback_stream(&spec, callback)
            .map_err(|e| anyhow::anyhow!("sdl open_playback_stream: {e}"))?;
        stream_with_cb
            .resume()
            .map_err(|e| anyhow::anyhow!("sdl resume: {e}"))?;

        log::info!("sdl audio: stream up at {TARGET_SAMPLE_RATE} Hz / {TARGET_CHANNELS}ch i16");
        Ok(Self {
            _stream: stream_with_cb,
            sample_rate: TARGET_SAMPLE_RATE as u32,
        })
    }
}
