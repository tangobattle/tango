use crate::audio;
use sdl2::audio::AudioFormatNum;

pub struct StreamWrapper(Box<dyn audio::Stream + Send + 'static>);

impl sdl2::audio::AudioCallback for StreamWrapper {
    type Channel = i16;

    fn callback(&mut self, buf: &mut [i16]) {
        let frame_count = self.0.fill(bytemuck::cast_slice_mut(buf));
        for x in &mut buf[frame_count * audio::NUM_CHANNELS..] {
            *x = Self::Channel::SILENCE;
        }
    }
}

pub struct Backend {
    _audio: sdl2::AudioSubsystem,
    audio_device: sdl2::audio::AudioDevice<StreamWrapper>,
}

impl Backend {
    pub fn new(sdl: &sdl2::Sdl, stream: impl audio::Stream + Send + 'static) -> Result<Self, anyhow::Error> {
        let audio = sdl.audio().map_err(|e| anyhow::format_err!("{}", e))?;
        let audio_device = audio
            .open_playback(
                None,
                &sdl2::audio::AudioSpecDesired {
                    freq: Some(48000),
                    channels: Some(audio::NUM_CHANNELS as u8),
                    samples: Some(audio::SAMPLES as u16),
                },
                |_| StreamWrapper(Box::new(stream)),
            )
            .map_err(|e| anyhow::format_err!("{}", e))?;
        log::info!("sdl2 audio spec: {:?}", audio_device.spec());
        audio_device.resume();
        Ok(Self {
            _audio: audio,
            audio_device,
        })
    }
}

impl audio::Backend for Backend {
    fn sample_rate(&self) -> u32 {
        self.audio_device.spec().freq as u32
    }
}
