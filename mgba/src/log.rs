extern "C" {
    fn vsnprintf(
        s: *mut std::os::raw::c_char,
        n: usize,
        format: *const std::os::raw::c_char,
        ap: mgba_sys::va_list,
    ) -> std::os::raw::c_int;
}

unsafe extern "C" fn c_log(
    _logger: *mut mgba_sys::mLogger,
    category: ::std::os::raw::c_int,
    level: mgba_sys::mLogLevel,
    fmt: *const std::os::raw::c_char,
    args: mgba_sys::va_list,
) {
    let level = match level {
        mgba_sys::mLogLevel_mLOG_STUB => log::Level::Trace,
        mgba_sys::mLogLevel_mLOG_DEBUG => log::Level::Debug,
        mgba_sys::mLogLevel_mLOG_INFO => log::Level::Info,
        mgba_sys::mLogLevel_mLOG_WARN => log::Level::Warn,
        mgba_sys::mLogLevel_mLOG_ERROR | mgba_sys::mLogLevel_mLOG_FATAL | mgba_sys::mLogLevel_mLOG_GAME_ERROR => {
            log::Level::Error
        }
        _ => log::Level::Info,
    };

    if !log::log_enabled!(level) {
        return;
    }

    // 4 KiB covers every mgba log line in practice; longer ones get truncated
    // rather than retried (va_list is consumed by vsnprintf, so a second pass
    // would need va_copy that we'd have to declare per platform).
    let mut buf = [0u8; 4096];
    let n = vsnprintf(buf.as_mut_ptr() as *mut _, buf.len(), fmt, args);
    let msg = if n < 0 {
        std::borrow::Cow::Borrowed("<vsnprintf failed>")
    } else {
        let written = (n as usize).min(buf.len() - 1);
        String::from_utf8_lossy(&buf[..written])
    };

    log::log!(
        level,
        "{}: {}",
        std::ffi::CStr::from_ptr(mgba_sys::mLogCategoryName(category)).to_string_lossy(),
        msg,
    );
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
