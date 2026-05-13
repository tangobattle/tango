#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct SyncRef<'a> {
    pub(super) ptr: *const mgba_sys::mCoreSync,
    pub(super) _lifetime: std::marker::PhantomData<&'a ()>,
}

impl<'a> SyncRef<'a> {
    pub fn fps_target(&self) -> f32 {
        unsafe { (*self.ptr).fpsTarget }
    }
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct SyncMutRef<'a> {
    pub(super) ptr: *mut mgba_sys::mCoreSync,
    pub(super) _lifetime: std::marker::PhantomData<&'a ()>,
}

impl<'a> SyncMutRef<'a> {
    pub fn as_ref(&self) -> SyncRef<'_> {
        SyncRef {
            ptr: self.ptr,
            _lifetime: std::marker::PhantomData,
        }
    }

    pub fn set_fps_target(&mut self, fps_target: f32) {
        unsafe {
            (*self.ptr).fpsTarget = fps_target;
        }
    }

    // mCoreSyncLoadCoreOpts pins audioHighWater at 512 frames, sized for the
    // 32 kHz default source rate. Battle Network games (and any title that
    // bumps SOUNDBIAS.resolution) push the GBA audio rate up to 65/131/262
    // kHz, at which point 512 source frames isn't enough to fill a single
    // host audio callback — the producer blocks every fill and the emulator
    // throttles below realtime (audible as low-pitched playback + underrun
    // crunch). Tango rescales this per fill mirroring mGBA's SDL frontend.
    pub fn set_audio_high_water(&mut self, frames: u32) {
        unsafe {
            (*self.ptr).audioHighWater = frames as _;
        }
    }
}
