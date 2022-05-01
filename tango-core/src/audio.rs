use cpal::traits::DeviceTrait;

pub mod mgba_stream;
pub mod mux_stream;

pub trait Stream {
    fn fill(&mut self, buf: &mut [i16]) -> usize;
}

pub fn get_supported_config(device: &cpal::Device) -> anyhow::Result<cpal::SupportedStreamConfig> {
    let mut supported_configs = device.supported_output_configs()?.collect::<Vec<_>>();
    supported_configs.sort_by_key(|x| {
        // Find the config that's closest to 2 channel 48000 Hz as we can.
        (
            x.max_sample_rate().0.abs_diff(48000),
            x.channels().abs_diff(2),
        )
    });

    let supported_config =
        if let Some(supported_config_range) = supported_configs.into_iter().next() {
            supported_config_range.with_max_sample_rate()
        } else {
            anyhow::bail!("no supported stream config found");
        };

    Ok(supported_config)
}

pub fn open_stream(
    device: &cpal::Device,
    supported_config: &cpal::SupportedStreamConfig,
    mut stream: impl Stream + Send + 'static,
) -> Result<cpal::Stream, anyhow::Error> {
    let error_callback = |err| log::error!("audio stream error: {}", err);
    let config = supported_config.config();
    let channels = config.channels;

    Ok(match supported_config.sample_format() {
        cpal::SampleFormat::U16 => device.build_output_stream(
            &config,
            {
                let mut buf = vec![];
                move |data: &mut [u16], _: &cpal::OutputCallbackInfo| {
                    if data.len() > buf.len() {
                        buf = vec![0i16; data.len() * 2];
                    }
                    let n = stream.fill(&mut buf[..data.len() / channels as usize * 2]);
                    realign_samples(&mut buf, channels);
                    for (x, y) in data.iter_mut().zip(buf[..n / 2 * channels as usize].iter()) {
                        *x = *y as u16 + 32768;
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
                    if data.len() > buf.len() {
                        buf = vec![0i16; data.len() * 2];
                    }
                    let n = stream.fill(&mut buf[..data.len() / channels as usize * 2]);
                    realign_samples(&mut buf, channels);
                    for (x, y) in data.iter_mut().zip(buf[..n / 2 * channels as usize].iter()) {
                        *x = *y;
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
                    if data.len() > buf.len() {
                        buf = vec![0i16; data.len() * 2];
                    }
                    let n = stream.fill(&mut buf[..data.len() / channels as usize * 2]);
                    realign_samples(&mut buf, channels);
                    for (x, y) in data.iter_mut().zip(buf[..n / 2 * channels as usize].iter()) {
                        *x = *y as f32 / 32768.0;
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
        let mut tmp = [0i16; 2];
        tmp.copy_from_slice(&buf[src..src + 2]);
        buf[src..src + 2].copy_from_slice(&[0, 0]);
        buf[dest..dest + 2].copy_from_slice(&tmp);
    }
}
