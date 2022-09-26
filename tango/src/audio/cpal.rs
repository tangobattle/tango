use crate::audio;
use cpal::traits::DeviceTrait;

fn get_supported_config(device: &cpal::Device) -> anyhow::Result<cpal::SupportedStreamConfig> {
    let mut supported_configs = device.supported_output_configs()?.collect::<Vec<_>>();
    supported_configs.sort_by_key(|x| {
        // Find the config that's closest to 2 channel 48000 Hz as we can.
        (x.max_sample_rate().0.abs_diff(48000), x.channels().abs_diff(2))
    });

    let supported_config = if let Some(supported_config_range) = supported_configs.into_iter().next() {
        supported_config_range.with_max_sample_rate()
    } else {
        anyhow::bail!("no supported stream config found");
    };

    Ok(supported_config)
}

fn open_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    mut stream: impl audio::Stream + Send + 'static,
) -> Result<cpal::Stream, anyhow::Error> {
    let error_callback = |err| log::error!("audio stream error: {}", err);
    let channels = config.channels;

    Ok(match sample_format {
        cpal::SampleFormat::U16 => device.build_output_stream(
            &config,
            {
                let mut buf = vec![];
                move |data: &mut [u16], _: &cpal::OutputCallbackInfo| {
                    if data.len() * 2 > buf.len() {
                        buf = vec![0i16; data.len() * 2];
                    }
                    let n = stream.fill(bytemuck::cast_slice_mut(
                        &mut buf[..data.len() / channels as usize * audio::NUM_CHANNELS],
                    ));
                    realign_samples(&mut buf, channels);
                    for (x, y) in data.iter_mut().zip(buf[..n * channels as usize].iter()) {
                        *x = (std::num::Wrapping(*y as u16) + std::num::Wrapping(32768)).0;
                    }
                    if data.len() > n * channels as usize {
                        for x in data[n * channels as usize..].iter_mut() {
                            *x = 32768;
                        }
                    }
                }
            },
            error_callback,
        ),
        cpal::SampleFormat::I16 => device.build_output_stream(
            &config,
            {
                let mut buf = vec![];
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    if data.len() * 2 > buf.len() {
                        buf = vec![0i16; data.len() * 2];
                    }
                    let n = stream.fill(bytemuck::cast_slice_mut(
                        &mut buf[..data.len() / channels as usize * audio::NUM_CHANNELS],
                    ));
                    realign_samples(&mut buf, channels);
                    for (x, y) in data.iter_mut().zip(buf[..n * channels as usize].iter()) {
                        *x = *y;
                    }
                    if data.len() > n * channels as usize {
                        for x in data[n * channels as usize..].iter_mut() {
                            *x = 0;
                        }
                    }
                }
            },
            error_callback,
        ),
        cpal::SampleFormat::F32 => device.build_output_stream(
            &config,
            {
                let mut buf = vec![];
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    if data.len() * 2 > buf.len() {
                        buf = vec![0i16; data.len() * 2];
                    }
                    let n = stream.fill(bytemuck::cast_slice_mut(
                        &mut buf[..data.len() / channels as usize * audio::NUM_CHANNELS],
                    ));
                    realign_samples(&mut buf, channels);
                    for (x, y) in data.iter_mut().zip(buf[..n * channels as usize].iter()) {
                        *x = *y as f32 / 32768.0;
                    }
                    if data.len() > n * channels as usize {
                        for x in data[n * channels as usize..].iter_mut() {
                            *x = 0.0;
                        }
                    }
                }
            },
            error_callback,
        ),
    }?)
}

fn realign_samples(buf: &mut [i16], channels: u16) {
    if channels == 2 {
        // On stereophonic audio, there is no realignment required.
        return;
    }

    // Monophonic downmix.
    if channels == 1 {
        for i in 0..(buf.len() / 2) {
            let l = buf[i as usize * 2] as i32;
            let r = buf[i as usize * 2 + 1] as i32;
            buf[i as usize] = ((l + r) / 2) as i16;
        }
        return;
    }

    // On layouts with >2 channels, we need to align the samples onto the first two channels.
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
}

impl Backend {
    pub fn new(stream: impl audio::Stream + Send + 'static) -> Result<Self, anyhow::Error> {
        use cpal::traits::{HostTrait, StreamTrait};

        let audio_device = cpal::default_host()
            .default_output_device()
            .ok_or_else(|| anyhow::format_err!("could not open audio device"))?;
        log::info!(
            "cpal supported audio output configs: {:?}",
            audio_device.supported_output_configs()?.collect::<Vec<_>>()
        );
        let audio_supported_config = get_supported_config(&audio_device)?;

        let mut config = audio_supported_config.config();
        match audio_supported_config.buffer_size() {
            cpal::SupportedBufferSize::Range { min, .. } => {
                config.buffer_size = cpal::BufferSize::Fixed(
                    std::cmp::max(
                        audio::SAMPLES as u32,
                        (*min + config.channels as u32 - 1) / config.channels as u32,
                    ) * config.channels as u32,
                );
            }
            cpal::SupportedBufferSize::Unknown => {
                log::warn!("supported buffer size is unknown, using default (it might be bad)")
            }
        }
        log::info!("selected audio config: {:?}", config);

        let stream = open_stream(&audio_device, &config, audio_supported_config.sample_format(), stream)?;
        stream.play()?;

        Ok(Self {
            _audio_device: audio_device,
            _stream: stream,
        })
    }
}

impl audio::Backend for Backend {}
