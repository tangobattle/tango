use crate::audio;

impl sdl2::audio::AudioCallback for dyn audio::Stream + Send {
    type Channel = i16;

    fn callback(&mut self, buf: &mut [i16]) {
        let frame_count = self.fill(bytemuck::cast_slice_mut(buf));
        for x in &mut buf[frame_count * audio::NUM_CHANNELS..] {
            use sdl2::audio::AudioFormatNum;
            *x = Self::Channel::SILENCE;
        }
    }
}
