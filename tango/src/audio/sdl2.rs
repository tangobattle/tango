use crate::audio;

pub struct StreamWrapper(Box<dyn audio::Stream + Send + 'static>);

impl sdl2::audio::AudioCallback for StreamWrapper {
    type Channel = i16;

    fn callback(&mut self, buf: &mut [i16]) {
        let frame_count = self.0.fill(bytemuck::cast_slice_mut(buf));
        for x in &mut buf[frame_count * audio::NUM_CHANNELS..] {
            use sdl2::audio::AudioFormatNum;
            *x = Self::Channel::SILENCE;
        }
    }
}

pub struct Backend {
    _audio_device: sdl2::audio::AudioDevice<StreamWrapper>,
}

impl Backend {
    pub fn new(
        audio: &sdl2::AudioSubsystem,
        stream: impl audio::Stream + Send + 'static,
    ) -> Result<Self, anyhow::Error> {
        let audio_device = audio
            .open_playback(
                None,
                &sdl2::audio::AudioSpecDesired {
                    freq: Some(48000),
                    channels: Some(audio::NUM_CHANNELS as u8),
                    samples: Some(512),
                },
                |_| StreamWrapper(Box::new(stream)),
            )
            .map_err(|e| anyhow::format_err!("{}", e))?;
        log::info!("sdl2 audio spec: {:?}", audio_device.spec());
        audio_device.resume();
        Ok(Self {
            _audio_device: audio_device,
        })
    }
}

impl audio::Backend for Backend {}
