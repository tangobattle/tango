use super::arm_core;
use super::sync;

pub const SCREEN_WIDTH: u32 = mgba_sys::GBA_VIDEO_HORIZONTAL_PIXELS;
pub const SCREEN_HEIGHT: u32 = mgba_sys::GBA_VIDEO_VERTICAL_PIXELS;

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
            _lifetime: self._lifetime,
        }
    }

    pub fn sync(&self) -> Option<sync::SyncRef> {
        let sync_ptr = unsafe { (*self.ptr).sync };
        if sync_ptr.is_null() {
            None
        } else {
            Some(sync::SyncRef {
                ptr: sync_ptr,
                _lifetime: self._lifetime,
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
    pub fn as_ref(&self) -> GBARef {
        GBARef {
            ptr: self.ptr,
            _lifetime: self._lifetime,
        }
    }

    pub fn cpu_mut(&self) -> arm_core::ARMCoreMutRef<'a> {
        arm_core::ARMCoreMutRef {
            ptr: unsafe { (*self.ptr).cpu },
            _lifetime: self._lifetime,
        }
    }

    pub fn sync_mut(&mut self) -> Option<sync::SyncMutRef> {
        let sync_ptr = unsafe { (*self.ptr).sync };
        if sync_ptr.is_null() {
            None
        } else {
            Some(sync::SyncMutRef {
                ptr: sync_ptr,
                _lifetime: self._lifetime,
            })
        }
    }
}

pub fn audio_calculate_ratio(
    input_sample_rate: f32,
    desired_fps: f32,
    desired_sample_rate: f32,
) -> f32 {
    unsafe { mgba_sys::GBAAudioCalculateRatio(input_sample_rate, desired_fps, desired_sample_rate) }
}
