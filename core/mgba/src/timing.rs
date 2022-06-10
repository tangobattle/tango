#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct TimingRef<'a> {
    pub(super) ptr: *const mgba_sys::mTiming,
    pub(super) _lifetime: std::marker::PhantomData<&'a ()>,
}

impl<'a> TimingRef<'a> {
    pub fn current_time(&self) -> i32 {
        unsafe { mgba_sys::mTimingCurrentTime(self.ptr) }
    }
}
