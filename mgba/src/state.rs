#[derive(Clone)]
#[repr(transparent)]
pub struct State(pub(super) mgba_sys::GBASerializedState);

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

    pub fn gpr(&self, i: usize) -> i32 {
        self.0.cpu.gprs[i]
    }

    pub fn cpsr(&self) -> i32 {
        unsafe { self.0.cpu.cpsr.packed }
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

    pub fn as_ptr(&self) -> *const u8 {
        &self.0 as *const _ as *const u8
    }

    pub fn as_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.as_ptr(), std::mem::size_of::<mgba_sys::GBASerializedState>()) }
    }

    pub fn new_uninit() -> Box<std::mem::MaybeUninit<Self>> {
        unsafe {
            let layout = std::alloc::Layout::new::<Self>();
            let ptr = std::alloc::alloc(layout);
            Box::from_raw(ptr as *mut _)
        }
    }

    pub fn from_slice(slice: &[u8]) -> Box<Self> {
        let mut state = Self::new_uninit();
        unsafe {
            std::slice::from_raw_parts_mut(
                state.as_mut_ptr() as *mut _,
                std::mem::size_of::<mgba_sys::GBASerializedState>(),
            )
            .copy_from_slice(slice);
            Box::from_raw(Box::into_raw(state) as *mut _)
        }
    }
}
