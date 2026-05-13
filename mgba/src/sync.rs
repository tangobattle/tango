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

    /// How many source-buffer frames must accumulate before
    /// `mCoreSyncProduceAudio` blocks the emulator. The default of 512 frames
    /// (set by mCoreSyncLoadCoreOpts) is sized for a 32 kHz source: at higher
    /// SOUNDBIAS resolutions the source rate jumps to 65/131/262 kHz, and a
    /// 512-frame threshold starves the audio thread of a single SDL
    /// callback's worth of samples. Tango rescales this per fill, mirroring
    /// what mGBA's SDL frontend does.
    pub fn set_audio_high_water(&mut self, frames: u32) {
        unsafe {
            (*self.ptr).audioHighWater = frames as _;
        }
    }
}
