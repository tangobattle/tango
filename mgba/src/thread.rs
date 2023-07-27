use super::{core, sync};

#[repr(transparent)]
pub struct InThreadHandle<'a> {
    raw: *mut mgba_sys::mCoreThread,
    _lifetime: std::marker::PhantomData<&'a ()>,
}

impl<'a> InThreadHandle<'a> {
    pub fn pause(&mut self) {
        unsafe { mgba_sys::mCoreThreadPauseFromThread(self.raw) }
    }
}

#[repr(transparent)]
pub struct Thread(std::sync::Arc<parking_lot::Mutex<Box<ThreadImpl>>>);

struct ThreadImpl {
    core: core::Core,
    _logger: Box<super::log::Logger>,
    raw: mgba_sys::mCoreThread,
    frame_callback: Option<Box<dyn Fn(core::CoreMutRef, &[u8], InThreadHandle) + Send + 'static>>,
    current_callback: std::cell::RefCell<Option<Box<dyn Fn(crate::core::CoreMutRef<'_>) + Send + Sync>>>,
}

unsafe impl Send for ThreadImpl {}

unsafe extern "C" fn c_frame_callback(ptr: *mut mgba_sys::mCoreThread) {
    let t = &mut *((*ptr).userData as *mut ThreadImpl);
    if let Some(cb) = t.frame_callback.as_ref() {
        cb(
            core::CoreMutRef {
                ptr: t.raw.core,
                _lifetime: std::marker::PhantomData,
            },
            t.core.video_buffer().unwrap(),
            InThreadHandle {
                raw: &mut t.raw,
                _lifetime: std::marker::PhantomData::<&'_ ()>,
            },
        );
    }
}

pub struct AudioGuard<'a> {
    sync: sync::SyncMutRef<'a>,
    thread: parking_lot::MutexGuard<'a, Box<ThreadImpl>>,
}

impl<'a> AudioGuard<'a> {
    pub fn core(&self) -> core::CoreRef<'a> {
        core::CoreRef {
            ptr: self.thread.raw.core,
            _lifetime: std::marker::PhantomData::<&'a ()>,
        }
    }

    pub fn core_mut(&mut self) -> core::CoreMutRef<'a> {
        core::CoreMutRef {
            ptr: self.thread.raw.core,
            _lifetime: std::marker::PhantomData::<&'a ()>,
        }
    }

    pub fn sync(&self) -> sync::SyncRef<'_> {
        self.sync.as_ref()
    }

    pub fn sync_mut(&self) -> sync::SyncMutRef<'_> {
        self.sync
    }
}

impl<'a> Drop for AudioGuard<'a> {
    fn drop(&mut self) {
        unsafe {
            mgba_sys::mCoreSyncConsumeAudio(self.sync.ptr);
        }
    }
}

impl Thread {
    pub fn new(core: core::Core) -> Self {
        let core_ptr = core.ptr;
        let logger = Box::new(super::log::Logger::new());
        let logger_ptr = logger.as_mlogger_ptr();
        let mut t = Box::new(ThreadImpl {
            core,
            raw: unsafe { std::mem::zeroed::<mgba_sys::mCoreThread>() },
            frame_callback: None,
            current_callback: std::cell::RefCell::new(None),
            _logger: logger,
        });
        t.raw.core = core_ptr;
        t.raw.logger.logger = logger_ptr as *mut _;
        t.raw.userData = &mut *t as *mut _ as *mut std::os::raw::c_void;
        t.raw.frameCallback = Some(c_frame_callback);
        Thread(std::sync::Arc::new(parking_lot::Mutex::new(t)))
    }

    pub fn set_frame_callback(&self, f: impl Fn(core::CoreMutRef, &[u8], InThreadHandle) + Send + 'static) {
        self.0.lock().frame_callback = Some(Box::new(f));
    }

    pub fn handle(&self) -> Handle {
        Handle { thread: self.0.clone() }
    }

    pub fn start(&self) -> Result<(), crate::Error> {
        if !unsafe { mgba_sys::mCoreThreadStart(&mut self.0.lock().raw) } {
            return Err(crate::Error::CallFailed("mCoreThreadStart"));
        }
        Ok(())
    }
}

impl Drop for ThreadImpl {
    fn drop(&mut self) {
        unsafe {
            mgba_sys::mCoreThreadEnd(&mut self.raw);
            mgba_sys::mCoreThreadJoin(&mut self.raw);
        }
    }
}

#[derive(Clone)]
pub struct Handle {
    thread: std::sync::Arc<parking_lot::Mutex<Box<ThreadImpl>>>,
}

unsafe extern "C" fn c_run_function(ptr: *mut mgba_sys::mCoreThread) {
    let t = &mut *((*ptr).userData as *mut ThreadImpl);
    let mut cc = t.current_callback.borrow_mut();
    let cc = cc.as_mut().unwrap();
    cc(crate::core::CoreMutRef {
        ptr: t.raw.core,
        _lifetime: std::marker::PhantomData,
    });
}

impl Handle {
    pub fn pause(&self) {
        let mut thread = self.thread.lock();
        unsafe { mgba_sys::mCoreThreadPause(&mut thread.raw) }
    }

    pub fn unpause(&self) {
        let mut thread = self.thread.lock();
        unsafe { mgba_sys::mCoreThreadUnpause(&mut thread.raw) }
    }

    pub fn is_paused(&self) -> bool {
        let mut thread = self.thread.lock();
        unsafe { mgba_sys::mCoreThreadIsPaused(&mut thread.raw) }
    }

    pub fn run_on_core(&self, f: impl Fn(crate::core::CoreMutRef<'_>) + Send + Sync + 'static) {
        let mut thread = self.thread.lock();
        *thread.current_callback.borrow_mut() = Some(Box::new(f));
        unsafe { mgba_sys::mCoreThreadRunFunction(&mut thread.raw, Some(c_run_function)) }
    }

    pub fn lock_audio(&self) -> AudioGuard {
        let mut thread = self.thread.lock();
        let sync = sync::SyncMutRef {
            ptr: unsafe { &mut (*thread.raw.impl_).sync as *mut _ },
            _lifetime: std::marker::PhantomData,
        };
        unsafe {
            mgba_sys::mCoreSyncLockAudio(sync.ptr);
        }
        AudioGuard { sync, thread }
    }

    pub fn has_crashed(&self) -> bool {
        let mut thread = self.thread.lock();
        unsafe { mgba_sys::mCoreThreadHasCrashed(&mut thread.raw) }
    }

    pub fn has_exited(&self) -> bool {
        let mut thread = self.thread.lock();
        unsafe { mgba_sys::mCoreThreadHasExited(&mut thread.raw) }
    }

    pub fn end(&self) {
        let mut thread = self.thread.lock();
        unsafe { mgba_sys::mCoreThreadEnd(&mut thread.raw) }
    }
}

unsafe impl Send for Handle {}

unsafe impl Sync for Handle {}
