use cpal::traits::DeviceTrait;

pub trait Stream {
    fn fill(&mut self, buf: &mut [[i16; NUM_CHANNELS]]) -> usize;
}

impl sdl2::audio::AudioCallback for dyn Stream + Send {
    type Channel = i16;

    fn callback(&mut self, buf: &mut [i16]) {
        let frame_count = self.fill(bytemuck::cast_slice_mut(buf));
        for x in &mut buf[frame_count * NUM_CHANNELS..] {
            use sdl2::audio::AudioFormatNum;
            *x = Self::Channel::SILENCE;
        }
    }
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
                    if data.len() * 2 > buf.len() {
                        buf = vec![0i16; data.len() * 2];
                    }
                    let n = stream.fill(bytemuck::cast_slice_mut(
                        &mut buf[..data.len() / channels as usize * NUM_CHANNELS],
                    ));
                    realign_samples(&mut buf, channels);
                    for (x, y) in data.iter_mut().zip(buf[..n * channels as usize].iter()) {
                        *x = (std::num::Wrapping(*y as u16) + std::num::Wrapping(32768)).0;
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
                        &mut buf[..data.len() / channels as usize * NUM_CHANNELS],
                    ));
                    realign_samples(&mut buf, channels);
                    for (x, y) in data.iter_mut().zip(buf[..n * channels as usize].iter()) {
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
                    if data.len() * 2 > buf.len() {
                        buf = vec![0i16; data.len() * 2];
                    }
                    let n = stream.fill(bytemuck::cast_slice_mut(
                        &mut buf[..data.len() / channels as usize * NUM_CHANNELS],
                    ));
                    realign_samples(&mut buf, channels);
                    for (x, y) in data.iter_mut().zip(buf[..n * channels as usize].iter()) {
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
        let mut tmp = [0i16; NUM_CHANNELS];
        tmp.copy_from_slice(&buf[src..src + 2]);
        buf[src..src + 2].copy_from_slice(&[0, 0]);
        buf[dest..dest + 2].copy_from_slice(&tmp);
    }
}

#[derive(thiserror::Error, Debug)]
pub enum BindingError {
    #[error("already bound")]
    AlreadyBound,
}

pub struct Binding {
    binder: LateBinder,
}

impl Drop for Binding {
    fn drop(&mut self) {
        *self.binder.stream.lock() = None;
    }
}

#[derive(Clone)]
pub struct LateBinder {
    supported_config: cpal::SupportedStreamConfig,
    stream: std::sync::Arc<parking_lot::Mutex<Option<Box<dyn Stream + Send + 'static>>>>,
}

impl LateBinder {
    pub fn new(supported_config: cpal::SupportedStreamConfig) -> Self {
        Self {
            supported_config,
            stream: std::sync::Arc::new(parking_lot::Mutex::new(None)),
        }
    }

    pub fn supported_config(&self) -> &cpal::SupportedStreamConfig {
        &self.supported_config
    }

    pub fn bind(
        &self,
        stream: Option<Box<dyn Stream + Send + 'static>>,
    ) -> Result<Binding, BindingError> {
        let mut stream_guard = self.stream.lock();
        if stream_guard.is_some() {
            return Err(BindingError::AlreadyBound);
        }

        *stream_guard = stream;
        Ok(Binding {
            binder: self.clone(),
        })
    }
}

impl Stream for LateBinder {
    fn fill(&mut self, buf: &mut [[i16; NUM_CHANNELS]]) -> usize {
        let mut stream = self.stream.lock();
        let stream = if let Some(stream) = &mut *stream {
            stream
        } else {
            for v in buf.iter_mut() {
                *v = [0, 0];
            }
            return buf.len() / 2;
        };
        stream.fill(buf)
    }
}

pub const NUM_CHANNELS: usize = 2;

pub struct MGBAStream {
    handle: mgba::thread::Handle,
    sample_rate: cpal::SampleRate,
}

impl MGBAStream {
    pub fn new(handle: mgba::thread::Handle, sample_rate: cpal::SampleRate) -> MGBAStream {
        Self {
            handle,
            sample_rate,
        }
    }
}

impl Stream for MGBAStream {
    fn fill(&mut self, buf: &mut [[i16; NUM_CHANNELS]]) -> usize {
        let frame_count = buf.len();
        let linear_buf = bytemuck::cast_slice_mut(buf);

        let mut audio_guard = self.handle.lock_audio();

        let mut fps_target = audio_guard.sync().fps_target();
        if fps_target <= 0.0 {
            fps_target = 1.0;
        }
        let faux_clock = mgba::gba::audio_calculate_ratio(1.0, fps_target, 1.0);

        let mut core = audio_guard.core_mut();

        let clock_rate = core.as_ref().frequency();

        let available = {
            let mut left = core.audio_channel(0);
            left.set_rates(
                clock_rate as f64,
                self.sample_rate.0 as f64 * faux_clock as f64,
            );
            let mut available = left.samples_avail() as usize;
            if available > frame_count {
                available = frame_count;
            }
            left.read_samples(linear_buf, available as i32, true);
            available
        };

        let mut right = core.audio_channel(1);
        right.set_rates(
            clock_rate as f64,
            self.sample_rate.0 as f64 * faux_clock as f64,
        );
        right.read_samples(&mut linear_buf[1..], available as i32, true);

        available as usize
    }
}
