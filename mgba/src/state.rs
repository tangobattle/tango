#[derive(Clone)]
#[repr(transparent)]
pub struct State(pub(super) Box<mgba_sys::GBASerializedState>);

unsafe impl Send for State {}

impl State {
    pub fn rom_title(&self) -> String {
        let title = unsafe { &*(&self.0.title as *const [std::os::raw::c_char] as *const [u8]) };
        let cstr = match std::ffi::CString::new(title) {
            Ok(r) => r,
            Err(err) => {
                let nul_pos = err.nul_position();
                std::ffi::CString::new(&err.into_vec()[0..nul_pos]).unwrap()
            }
        };
        cstr.to_str().unwrap().to_string()
    }

    pub fn gpr(&self, i: usize) -> u32 {
        self.0.cpu.gprs[i] as u32
    }

    pub fn rom_crc32(&self) -> u32 {
        self.0.romCrc32
    }

    pub fn wram(&self) -> &[u8] {
        &self.0.wram
    }

    pub fn iwram(&self) -> &[u8] {
        &self.0.iwram
    }

    pub fn oam(&self) -> &[u16] {
        &self.0.oam
    }

    pub fn pram(&self) -> &[u16] {
        &self.0.pram
    }

    pub fn as_slice(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                &*self.0 as *const mgba_sys::GBASerializedState as *const u8,
                std::mem::size_of::<mgba_sys::GBASerializedState>(),
            )
        }
    }

    pub fn from_slice(slice: &[u8]) -> Self {
        unsafe {
            let layout = std::alloc::Layout::new::<mgba_sys::GBASerializedState>();
            let ptr = std::alloc::alloc(layout);
            if ptr.is_null() {
                std::alloc::handle_alloc_error(layout);
            }
            let slice2 = std::slice::from_raw_parts_mut(
                ptr,
                std::mem::size_of::<mgba_sys::GBASerializedState>(),
            );
            slice2.copy_from_slice(slice);
            Self(Box::from_raw(
                ptr as *mut _ as *mut mgba_sys::GBASerializedState,
            ))
        }
    }
}
