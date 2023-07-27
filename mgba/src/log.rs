unsafe extern "C" fn c_log<VaList>(
    _logger: *mut mgba_sys::mLogger,
    category: i32,
    level: u32,
    fmt: *const std::os::raw::c_char,
    args: VaList,
) {
    // log::log!(
    //     match level {
    //         mgba_sys::mLogLevel_mLOG_STUB => log::Level::Trace,
    //         mgba_sys::mLogLevel_mLOG_DEBUG => log::Level::Debug,
    //         mgba_sys::mLogLevel_mLOG_INFO => log::Level::Info,
    //         mgba_sys::mLogLevel_mLOG_WARN => log::Level::Warn,
    //         mgba_sys::mLogLevel_mLOG_ERROR | mgba_sys::mLogLevel_mLOG_FATAL | mgba_sys::mLogLevel_mLOG_GAME_ERROR =>
    //             log::Level::Error,
    //         _ => log::Level::Info,
    //     },
    //     "{}: {}",
    //     std::ffi::CStr::from_ptr(mgba_sys::mLogCategoryName(category)).to_string_lossy(),
    //     todo!()
    // );
}

pub struct Logger {
    logger: mgba_sys::mLogger,
    _log_filter: Box<mgba_sys::mLogFilter>,
}
unsafe impl Sync for Logger {}
unsafe impl Send for Logger {}

impl Logger {
    pub fn new() -> Logger {
        let log_filter = unsafe {
            let mut log_filter = Box::new(std::mem::zeroed::<mgba_sys::mLogFilter>());
            mgba_sys::mLogFilterInit(log_filter.as_mut() as *mut _);
            log_filter
        };

        Self {
            logger: mgba_sys::mLogger {
                log: Some(c_log),
                filter: log_filter.as_ref() as *const _ as *mut _,
            },
            _log_filter: log_filter,
        }
    }

    pub fn as_mlogger_ptr(&self) -> *const mgba_sys::mLogger {
        &self.logger as *const _
    }
}
