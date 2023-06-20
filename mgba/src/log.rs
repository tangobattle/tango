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

#[repr(transparent)]
struct LogFilter(mgba_sys::mLogFilter);
unsafe impl Sync for LogFilter {}
unsafe impl Send for LogFilter {}

#[repr(transparent)]
struct Logger(mgba_sys::mLogger);
unsafe impl Sync for Logger {}
unsafe impl Send for Logger {}

pub(crate) fn init() {
    static LOGGER: once_cell::sync::Lazy<Logger> = once_cell::sync::Lazy::new(|| {
        static LOG_FILTER: once_cell::sync::Lazy<LogFilter> = once_cell::sync::Lazy::new(|| unsafe {
            let mut log_filter = std::mem::zeroed::<mgba_sys::mLogFilter>();
            mgba_sys::mLogFilterInit(&mut log_filter);
            LogFilter(log_filter)
        });

        Logger(mgba_sys::mLogger {
            log: Some(c_log),
            filter: &LOG_FILTER.0 as *const _ as *mut _,
        })
    });

    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| unsafe {
        mgba_sys::mLogSetDefaultLogger(&LOGGER.0 as *const _ as *mut _);
    });
}
