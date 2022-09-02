use crate::audio;

pub struct StreamWrapper<T>(T);

impl<T> sdl2::audio::AudioCallback for StreamWrapper<T>
where
    T: audio::Stream + Send,
{
    type Channel = i16;

    fn callback(&mut self, buf: &mut [i16]) {
        let frame_count = self.0.fill(bytemuck::cast_slice_mut(buf));
        for x in &mut buf[frame_count * audio::NUM_CHANNELS..] {
            use sdl2::audio::AudioFormatNum;
            *x = Self::Channel::SILENCE;
        }
    }
}

pub fn open_stream<T>(
    audio: &sdl2::AudioSubsystem,
    spec: &sdl2::audio::AudioSpecDesired,
    stream: T,
) -> Result<sdl2::audio::AudioDevice<StreamWrapper<T>>, String>
where
    T: audio::Stream + Send,
{
    audio.open_playback(None, spec, |_| StreamWrapper(stream))
}
