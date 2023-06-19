use const_zero::const_zero;

lazy_static! {
    static ref MLOG_FILTER: send_wrapper::SendWrapper<parking_lot::Mutex<mgba_sys::mLogFilter>> = {
        let mut ptr = unsafe { const_zero!(mgba_sys::mLogFilter) };
        unsafe {
            mgba_sys::mLogFilterInit(&mut ptr);
        }
        send_wrapper::SendWrapper::new(parking_lot::Mutex::new(ptr))
    };
    static ref MLOGGER: send_wrapper::SendWrapper<parking_lot::Mutex<mgba_sys::mLogger>> =
        send_wrapper::SendWrapper::new(parking_lot::Mutex::new(mgba_sys::mLogger {
            log: Some(c_log),
            filter: &mut *MLOG_FILTER.lock(),
        }));
    static ref LOG_FUNC: send_wrapper::SendWrapper<parking_lot::Mutex<Box<dyn Fn(i32, u32, String)>>> =
        send_wrapper::SendWrapper::new(parking_lot::Mutex::new(Box::new(&|category, _level, message| {
            let category_name = unsafe { std::ffi::CStr::from_ptr(mgba_sys::mLogCategoryName(category)) }
                .to_str()
                .unwrap();
            log::info!("{}: {}", category_name, message);
        })));
}

unsafe extern "C" fn c_log<VaList>(
    _logger: *mut mgba_sys::mLogger,
    category: i32,
    level: u32,
    fmt: *const std::os::raw::c_char,
    args: VaList,
) {
    const INITIAL_BUF_SIZE: usize = 512;
    let mut buf = vec![0u8; INITIAL_BUF_SIZE];

    let mut done = false;
    while !done {
        let n: usize = mgba_sys::vsnprintf(
            buf.as_mut_ptr() as *mut _,
            buf.len() as u64,
            fmt,
            std::mem::transmute_copy(&args),
        )
        .try_into()
        .unwrap();

        done = n + 1 <= buf.len();
        buf.resize(n + 1, 0);
    }

    let cstr = match std::ffi::CString::new(buf) {
        Ok(r) => r,
        Err(err) => {
            let nul_pos = err.nul_position();
            std::ffi::CString::new(&err.into_vec()[0..nul_pos]).unwrap()
        }
    };

    LOG_FUNC.lock().as_ref()(category, level, cstr.to_string_lossy().to_string());
}

pub fn init() {
    unsafe {
        mgba_sys::mLogSetDefaultLogger(&mut *MLOGGER.lock());
    }
}
