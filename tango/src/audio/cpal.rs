//! cpal-backed audio output. Picks the default output device, opens a
//! stream at (or near) 48 kHz stereo, and pumps whatever `Stream` you
//! hand it. The legacy app supported SDL2 as a fallback; we don't (yet).
//!
//! Ported from `tango/src/audio/cpal.rs`.

use crate::audio;
use cpal::{traits::DeviceTrait, Sample};

fn get_supported_config(device: &cpal::Device) -> anyhow::Result<cpal::SupportedStreamConfig> {
    let mut supported_configs = device.supported_output_configs()?.collect::<Vec<_>>();
    // Closest to 2-channel 48 kHz wins.
    supported_configs.sort_by_key(|x| (x.max_sample_rate().0.abs_diff(48000), x.channels().abs_diff(2)));
    let cfg = supported_configs
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("no supported audio output config"))?;
    Ok(cfg.with_max_sample_rate())
}

fn make_data_callback<T>(
    mut stream: impl audio::Stream + Send + 'static,
    channels: u16,
) -> impl FnMut(&mut [T], &cpal::OutputCallbackInfo) + Send + 'static
where
    T: cpal::Sample + cpal::FromSample<i16>,
{
    let mut buf = vec![];
    move |data, _| {
        if data.len() * 2 > buf.len() {
            buf = vec![0i16; data.len() * 2];
        }
        let n = stream.fill(bytemuck::cast_slice_mut(
            &mut buf[..data.len() / channels as usize * audio::NUM_CHANNELS],
        ));
        realign_samples(&mut buf, channels);
        for (dst, src) in std::iter::zip(
            data.iter_mut(),
            buf[..n * channels as usize]
                .iter()
                .map(|v| v.to_sample())
                .chain(std::iter::repeat(T::EQUILIBRIUM)),
        ) {
            *dst = src;
        }
    }
}

fn open_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    stream: impl audio::Stream + Send + 'static,
) -> anyhow::Result<cpal::Stream> {
    let error_callback = |err| log::error!("audio stream error: {err}");
    let channels = config.channels;

    macro_rules! build {
        ($t:ty) => {
            device.build_output_stream(
                config,
                make_data_callback::<$t>(stream, channels),
                error_callback,
                None,
            )
        };
    }

    Ok(match sample_format {
        cpal::SampleFormat::U8 => build!(u8),
        cpal::SampleFormat::U16 => build!(u16),
        cpal::SampleFormat::U32 => build!(u32),
        cpal::SampleFormat::U64 => build!(u64),
        cpal::SampleFormat::I8 => build!(i8),
        cpal::SampleFormat::I16 => build!(i16),
        cpal::SampleFormat::I32 => build!(i32),
        cpal::SampleFormat::I64 => build!(i64),
        cpal::SampleFormat::F32 => build!(f32),
        cpal::SampleFormat::F64 => build!(f64),
        _ => anyhow::bail!("unsupported cpal sample format: {sample_format}"),
    }?)
}

/// Mono / surround systems still get 2-channel i16 from the resampler;
/// fix up the layout in-place to match `channels`.
fn realign_samples(buf: &mut [i16], channels: u16) {
    if channels == 2 {
        return;
    }
    if channels == 1 {
        for i in 0..(buf.len() / 2) {
            let l = buf[i * 2] as i32;
            let r = buf[i * 2 + 1] as i32;
            buf[i] = ((l + r) / 2) as i16;
        }
        return;
    }
    // Surround: scatter L/R into the first two channels of each frame
    // and zero the rest. Walking back-to-front so the source samples
    // aren't clobbered before they're read.
    for i in (1..(buf.len() / channels as usize)).rev() {
        let src = i * 2;
        let dest = i * channels as usize;
        let mut tmp = [0i16; audio::NUM_CHANNELS];
        tmp.copy_from_slice(&buf[src..src + 2]);
        buf[src..src + 2].copy_from_slice(&[0, 0]);
        buf[dest..dest + 2].copy_from_slice(&tmp);
    }
}

pub struct Backend {
    _audio_device: cpal::Device,
    _stream: cpal::Stream,
    sample_rate: cpal::SampleRate,
}

impl Backend {
    pub fn new(stream: impl audio::Stream + Send + 'static) -> anyhow::Result<Self> {
        use cpal::traits::{HostTrait, StreamTrait};
        let device = cpal::default_host()
            .default_output_device()
            .ok_or_else(|| anyhow::anyhow!("no default audio output device"))?;
        let supported = get_supported_config(&device)?;
        let mut config = supported.config();
        // Request the small low-latency buffer cpal supports. Windows
        // WASAPI honors this; Linux PipeWire / PulseAudio largely
        // ignore it and serve their own quantum. That's OK — the
        // MGBAStream destination buffer grows on demand to fit
        // whatever frame count actually arrives in the callback.
        config.buffer_size = cpal::BufferSize::Fixed(audio::SAMPLES as u32);
        log::info!("cpal: selected audio config {config:?}");

        let s = open_stream(&device, &config, supported.sample_format(), stream)?;
        s.play()?;

        Ok(Self {
            _audio_device: device,
            _stream: s,
            sample_rate: config.sample_rate,
        })
    }
}

impl audio::Backend for Backend {
    fn sample_rate(&self) -> u32 {
        self.sample_rate.0
    }
}
