use cpal::traits::DeviceTrait;

pub mod mux_stream;
pub mod timewarp_stream;

pub trait Stream {
    fn fill(&self, buf: &mut [i16]) -> usize;
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
    stream: impl Stream + Send + 'static,
) -> Result<cpal::Stream, anyhow::Error> {
    let error_callback = |err| log::error!("audio stream error: {}", err);

    Ok(match supported_config.sample_format() {
        cpal::SampleFormat::U16 => device.build_output_stream(
            &supported_config.config(),
            {
                let mut buf = vec![];
                move |data: &mut [u16], _: &cpal::OutputCallbackInfo| {
                    if data.len() > buf.len() {
                        buf = vec![0i16; data.len()];
                    }
                    let n = stream.fill(&mut buf[..data.len()]);
                    for (x, y) in data.iter_mut().zip(buf[..n].iter()) {
                        *x = *y as u16 + 32768;
                    }
                }
            },
            error_callback,
        ),
        cpal::SampleFormat::I16 => device.build_output_stream(
            &supported_config.config(),
            {
                let mut buf = vec![];
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    if data.len() > buf.len() {
                        buf = vec![0i16; data.len()];
                    }
                    let n = stream.fill(&mut buf[..data.len()]);
                    for (x, y) in data.iter_mut().zip(buf[..n].iter()) {
                        *x = *y;
                    }
                }
            },
            error_callback,
        ),
        cpal::SampleFormat::F32 => device.build_output_stream(
            &supported_config.config(),
            {
                let mut buf = vec![];
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    if data.len() > buf.len() {
                        buf = vec![0i16; data.len()];
                    }
                    let n = stream.fill(&mut buf[..data.len()]);
                    for (x, y) in data.iter_mut().zip(buf[..n].iter()) {
                        *x = *y as f32 / 32768.0;
                    }
                }
            },
            error_callback,
        ),
    }?)
}
