use super::blip;
use super::gba;
use super::state;
use super::trapper;
use super::vfile;
use std::ffi::CString;

pub struct Core {
    pub(super) ptr: *mut mgba_sys::mCore,
    video_buffer: Option<Vec<u8>>,
    trapper: Option<trapper::Trapper>,
}

unsafe impl Send for Core {}

impl Core {
    pub fn new_gba(config_name: &str) -> Result<Self, crate::Error> {
        super::log::init();

        let ptr = unsafe { mgba_sys::GBACoreCreate() };
        if ptr.is_null() {
            return Err(crate::Error::CallFailed("GBACoreCreate"));
        }
        unsafe {
            {
                // TODO: Make this more generic maybe.
                let opts = &mut ptr.as_mut().unwrap().opts;
                opts.sampleRate = 48000;
                opts.videoSync = false;
                opts.audioSync = true;
            }

            (*ptr).init.unwrap()(ptr);
            let config_name_cstr = CString::new(config_name).unwrap();
            mgba_sys::mCoreConfigInit(&mut ptr.as_mut().unwrap().config, config_name_cstr.as_ptr());
            mgba_sys::mCoreConfigLoad(&mut ptr.as_mut().unwrap().config);
        }

        Ok(Core {
            ptr,
            video_buffer: None,
            trapper: None,
        })
    }

    pub fn enable_video_buffer(&mut self) {
        let (width, height) = self.as_ref().desired_video_dimensions();
        let mut buffer = vec![0u8; (width * height * 4) as usize];
        unsafe {
            (*self.ptr).setVideoBuffer.unwrap()(
                self.ptr,
                buffer.as_mut_ptr() as *mut _ as *mut u32,
                width as mgba_sys::size_t,
            );
        }
        self.video_buffer = Some(buffer);
    }

    pub fn as_ref(&self) -> CoreRef {
        CoreRef {
            ptr: self.ptr,
            _lifetime: std::marker::PhantomData,
        }
    }

    pub fn as_mut(&mut self) -> CoreMutRef {
        CoreMutRef {
            ptr: self.ptr,
            _lifetime: std::marker::PhantomData,
        }
    }

    pub fn video_buffer(&self) -> Option<&[u8]> {
        self.video_buffer.as_deref()
    }

    pub fn set_traps(&mut self, traps: Vec<(u32, Box<dyn Fn(CoreMutRef)>)>) {
        self.trapper = Some(trapper::Trapper::new(self.as_mut(), traps));
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        unsafe {
            mgba_sys::mCoreConfigDeinit(&mut self.ptr.as_mut().unwrap().config);
            (*self.ptr).deinit.unwrap()(self.ptr)
        }
    }
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct CoreRef<'a> {
    pub(super) ptr: *const mgba_sys::mCore,
    pub(super) _lifetime: std::marker::PhantomData<&'a ()>,
}

unsafe impl<'a> Send for CoreRef<'a> {}

impl<'a> CoreRef<'a> {
    pub fn frequency(&self) -> i32 {
        unsafe { (*self.ptr).frequency.unwrap()(self.ptr) }
    }

    pub fn desired_video_dimensions(&self) -> (u32, u32) {
        let mut width: u32 = 0;
        let mut height: u32 = 0;
        unsafe { (*self.ptr).desiredVideoDimensions.unwrap()(self.ptr, &mut width, &mut height) };
        (width, height)
    }

    pub fn gba(&self) -> gba::GBARef {
        gba::GBARef {
            ptr: unsafe { (*self.ptr).board as *const mgba_sys::GBA },
            _lifetime: std::marker::PhantomData,
        }
    }

    pub fn game_title(&self) -> String {
        let mut title = [0u8; 16];
        unsafe {
            (*self.ptr).getGameTitle.unwrap()(self.ptr, title.as_mut_ptr() as *mut _ as *mut std::os::raw::c_char)
        }
        let cstr = match std::ffi::CString::new(title) {
            Ok(r) => r,
            Err(err) => {
                let nul_pos = err.nul_position();
                std::ffi::CString::new(&err.into_vec()[0..nul_pos]).unwrap()
            }
        };
        cstr.to_string_lossy().to_string()
    }

    pub fn game_code(&self) -> String {
        let mut code = [0u8; 12];
        unsafe { (*self.ptr).getGameCode.unwrap()(self.ptr, code.as_mut_ptr() as *mut _ as *mut std::os::raw::c_char) }
        let cstr = match std::ffi::CString::new(code) {
            Ok(r) => r,
            Err(err) => {
                let nul_pos = err.nul_position();
                std::ffi::CString::new(&err.into_vec()[0..nul_pos]).unwrap()
            }
        };
        cstr.to_string_lossy().to_string()
    }

    pub fn crc32(&self) -> u32 {
        let mut c: u32 = 0;
        unsafe {
            (*self.ptr).checksum.unwrap()(
                self.ptr,
                &mut c as *mut _ as *mut std::ffi::c_void,
                mgba_sys::mCoreChecksumType_mCHECKSUM_CRC32,
            )
        };
        c
    }

    pub fn frame_counter(&self) -> u32 {
        unsafe { (*self.ptr).frameCounter.unwrap()(self.ptr) }
    }
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct CoreMutRef<'a> {
    pub(super) ptr: *mut mgba_sys::mCore,
    pub(super) _lifetime: std::marker::PhantomData<&'a ()>,
}

unsafe impl<'a> Send for CoreMutRef<'a> {}

impl<'a> CoreMutRef<'a> {
    pub fn as_ref(&self) -> CoreRef {
        CoreRef {
            ptr: self.ptr,
            _lifetime: self._lifetime,
        }
    }

    pub fn gba_mut(&mut self) -> gba::GBAMutRef {
        gba::GBAMutRef {
            ptr: unsafe { (*self.ptr).board as *mut mgba_sys::GBA },
            _lifetime: std::marker::PhantomData,
        }
    }

    pub fn load_rom(&mut self, mut vf: vfile::VFile) -> Result<(), crate::Error> {
        if !unsafe { (*self.ptr).loadROM.unwrap()(self.ptr, vf.release()) } {
            return Err(crate::Error::CallFailed("mCore.loadROM"));
        }
        Ok(())
    }

    pub fn load_save(&mut self, mut vf: vfile::VFile) -> Result<(), crate::Error> {
        if !unsafe { (*self.ptr).loadSave.unwrap()(self.ptr, vf.release()) } {
            return Err(crate::Error::CallFailed("mCore.loadSave"));
        }
        Ok(())
    }

    pub fn load_state(&mut self, state: &state::State) -> Result<(), crate::Error> {
        if !unsafe { (*self.ptr).loadState.unwrap()(self.ptr, state.as_ptr() as *const _) } {
            return Err(crate::Error::CallFailed("mCore.loadState"));
        }
        Ok(())
    }

    pub fn save_state(&self) -> Result<Box<state::State>, crate::Error> {
        unsafe {
            let layout = std::alloc::Layout::new::<mgba_sys::GBASerializedState>();
            let ptr = std::alloc::alloc(layout);
            if ptr.is_null() {
                std::alloc::handle_alloc_error(layout);
            }
            let mut state = state::State::new_uninit();
            if !(*self.ptr).saveState.unwrap()(self.ptr, state.as_mut_ptr() as *mut _) {
                return Err(crate::Error::CallFailed("mCore.saveState"));
            }
            Ok(Box::from_raw(Box::into_raw(state) as *mut _))
        }
    }

    pub fn set_keys(&mut self, keys: u32) {
        unsafe { (*self.ptr).setKeys.unwrap()(self.ptr, keys) }
    }

    pub fn raw_read_8(&mut self, address: u32, segment: i32) -> u8 {
        unsafe { (*self.ptr).rawRead8.unwrap()(self.ptr, address, segment) as u8 }
    }

    pub fn raw_read_16(&mut self, address: u32, segment: i32) -> u16 {
        unsafe { (*self.ptr).rawRead16.unwrap()(self.ptr, address, segment) as u16 }
    }

    pub fn raw_read_32(&mut self, address: u32, segment: i32) -> u32 {
        unsafe { (*self.ptr).rawRead32.unwrap()(self.ptr, address, segment) as u32 }
    }

    pub fn raw_read_range(&mut self, address: u32, segment: i32, buf: &mut [u8]) {
        for (i, v) in buf.iter_mut().enumerate() {
            *v = self.raw_read_8(address + i as u32, segment);
        }
    }

    pub fn raw_write_8(&mut self, address: u32, segment: i32, v: u8) {
        unsafe { (*self.ptr).rawWrite8.unwrap()(self.ptr, address, segment, v) }
    }

    pub fn raw_write_16(&mut self, address: u32, segment: i32, v: u16) {
        unsafe { (*self.ptr).rawWrite16.unwrap()(self.ptr, address, segment, v) }
    }

    pub fn raw_write_32(&mut self, address: u32, segment: i32, v: u32) {
        unsafe { (*self.ptr).rawWrite32.unwrap()(self.ptr, address, segment, v) }
    }

    pub fn raw_write_range(&mut self, address: u32, segment: i32, buf: &[u8]) {
        for (i, v) in buf.iter().enumerate() {
            self.raw_write_8(address + i as u32, segment, *v);
        }
    }

    pub fn run_frame(&mut self) {
        unsafe { (*self.ptr).runFrame.unwrap()(self.ptr) }
    }

    pub fn run_loop(&mut self) {
        unsafe { (*self.ptr).runLoop.unwrap()(self.ptr) }
    }

    pub fn step(&mut self) {
        unsafe { (*self.ptr).step.unwrap()(self.ptr) }
    }

    pub fn reset(&mut self) {
        unsafe { (*self.ptr).reset.unwrap()(self.ptr) }
    }

    pub fn audio_buffer_size(&mut self) -> u64 {
        unsafe { (*self.ptr).getAudioBufferSize.unwrap()(self.ptr) }
    }

    pub fn set_audio_buffer_size(&mut self, size: u64) {
        unsafe { (*self.ptr).setAudioBufferSize.unwrap()(self.ptr, size) }
    }

    pub fn audio_channel(&mut self, ch: i32) -> blip::BlipMutRef {
        blip::BlipMutRef {
            ptr: unsafe { (*self.ptr).getAudioChannel.unwrap()(self.ptr, ch) },
            _lifetime: std::marker::PhantomData,
        }
    }

    pub unsafe fn from_ptr(ptr: *mut mgba_sys::mCore) -> Self {
        CoreMutRef {
            ptr,
            _lifetime: std::marker::PhantomData,
        }
    }
}
