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
    pub fn as_ref(&self) -> SyncRef {
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
}
