use super::arm_core;
use super::sync;
use super::timing;

pub const SCREEN_WIDTH: u32 = mgba_sys::GBA_VIDEO_HORIZONTAL_PIXELS as u32;
pub const SCREEN_HEIGHT: u32 = mgba_sys::GBA_VIDEO_VERTICAL_PIXELS as u32;

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct GBARef<'a> {
    pub(super) ptr: *const mgba_sys::GBA,
    pub(super) _lifetime: std::marker::PhantomData<&'a ()>,
}

impl<'a> GBARef<'a> {
    pub fn cpu(&self) -> arm_core::ARMCoreRef<'a> {
        arm_core::ARMCoreRef {
            ptr: unsafe { (*self.ptr).cpu },
            _lifetime: std::marker::PhantomData,
        }
    }

    pub fn timing(&self) -> timing::TimingRef<'_> {
        timing::TimingRef {
            ptr: unsafe { &(*self.ptr).timing },
            _lifetime: std::marker::PhantomData,
        }
    }

    pub fn master_volume(&self) -> i32 {
        unsafe { (*self.ptr).audio.masterVolume }
    }

    pub fn sync(&self) -> Option<sync::SyncRef<'_>> {
        let sync_ptr = unsafe { (*self.ptr).sync };
        if sync_ptr.is_null() {
            None
        } else {
            Some(sync::SyncRef {
                ptr: sync_ptr,
                _lifetime: std::marker::PhantomData,
            })
        }
    }
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct GBAMutRef<'a> {
    pub(super) ptr: *mut mgba_sys::GBA,
    pub(super) _lifetime: std::marker::PhantomData<&'a ()>,
}

impl<'a> GBAMutRef<'a> {
    pub fn as_ref(&self) -> GBARef<'_> {
        GBARef {
            ptr: self.ptr,
            _lifetime: std::marker::PhantomData,
        }
    }

    pub fn set_master_volume(&self, volume: i32) {
        unsafe {
            (*self.ptr).audio.masterVolume = volume;
        }
    }

    /// Set the video frameskip. A large value makes the GBA video module skip
    /// `drawScanline` + `finishFrame` outright (both gated on
    /// `frameskipCounter <= 0` in gba/video.c), so the core advances game logic
    /// without rasterizing. Used on the headless fast-forward and shadow cores,
    /// whose pixels are never shown — the display core re-renders from the save
    /// states they capture, and VRAM/IO are driven by the CPU, not the renderer.
    /// `frameskip` isn't part of the serialized state, so loading a save state
    /// (e.g. one captured on a rendering core) won't clear it.
    pub fn set_frameskip(&self, frameskip: i32) {
        unsafe {
            (*self.ptr).video.frameskip = frameskip;
            (*self.ptr).video.frameskipCounter = frameskip;
        }
    }

    pub fn cpu_mut(&self) -> arm_core::ARMCoreMutRef<'a> {
        arm_core::ARMCoreMutRef {
            ptr: unsafe { (*self.ptr).cpu },
            _lifetime: std::marker::PhantomData,
        }
    }

    pub fn sync_mut(&mut self) -> Option<sync::SyncMutRef<'_>> {
        let sync_ptr = unsafe { (*self.ptr).sync };
        if sync_ptr.is_null() {
            None
        } else {
            Some(sync::SyncMutRef {
                ptr: sync_ptr,
                _lifetime: std::marker::PhantomData,
            })
        }
    }
}
