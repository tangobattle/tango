//! cpal output backend for the [`LateBinder`] — the mobile-safe
//! replacement for the old frontend's SDL backend (AAudio on Android,
//! CoreAudio on macOS/iOS, WASAPI on Windows).
//!
//! We keep the device's native sample rate: `MGBAStream` resamples to
//! whatever rate the binder reports, so there's nothing to force here.

use anyhow::Context as _;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use super::{LateBinder, Stream as _, NUM_CHANNELS};

pub struct Backend {
    _stream: cpal::Stream,
}

impl Backend {
    pub fn new(binder: &mut LateBinder) -> anyhow::Result<Self> {
        let host = cpal::default_host();
        let device = host.default_output_device().context("no audio output device")?;
        let default_config = device.default_output_config().context("no default output config")?;

        let config = cpal::StreamConfig {
            channels: NUM_CHANNELS as cpal::ChannelCount,
            sample_rate: default_config.sample_rate(),
            buffer_size: cpal::BufferSize::Default,
        };
        binder.set_sample_rate(config.sample_rate);
        log::info!(
            "audio: default output device @ {} Hz, {:?}",
            config.sample_rate,
            default_config.sample_format(),
        );

        let err_fn = |e| log::warn!("audio stream error: {e}");
        let stream = match default_config.sample_format() {
            cpal::SampleFormat::I16 => device.build_output_stream(
                config,
                {
                    let mut binder = binder.clone();
                    move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                        let frames: &mut [[i16; NUM_CHANNELS]] = bytemuck::cast_slice_mut(data);
                        let n = binder.fill(frames);
                        frames[n..].fill([0; NUM_CHANNELS]);
                    }
                },
                err_fn,
                None,
            )?,
            cpal::SampleFormat::F32 => device.build_output_stream(
                config,
                {
                    let mut binder = binder.clone();
                    let mut scratch: Vec<[i16; NUM_CHANNELS]> = Vec::new();
                    move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                        let frames = data.len() / NUM_CHANNELS;
                        scratch.resize(frames, [0; NUM_CHANNELS]);
                        let n = binder.fill(&mut scratch);
                        scratch[n..].fill([0; NUM_CHANNELS]);
                        for (dst, src) in data.chunks_exact_mut(NUM_CHANNELS).zip(scratch.iter()) {
                            for (d, s) in dst.iter_mut().zip(src.iter()) {
                                *d = *s as f32 / 32768.0;
                            }
                        }
                    }
                },
                err_fn,
                None,
            )?,
            f => anyhow::bail!("unsupported sample format: {f:?}"),
        };
        stream.play()?;

        Ok(Self { _stream: stream })
    }
}
