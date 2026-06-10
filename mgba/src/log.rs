use std::mem::MaybeUninit;

// On targets where the C ABI defines `va_list` as a single-element array,
// C decays it to a pointer when used as a function parameter, and bindgen
// reflects that decay in function-pointer signatures (the `mLogger.log`
// field ends up `*mut <element>`). Using the raw `va_list` typedef both
// fails to match that signature and would pass the wrong thing to
// `vsnprintf` by ABI. Two such ABIs are in play:
//   - SysV AMD64 (x86_64 Linux/macOS/BSD): `va_list = [__va_list_tag; 1]`
//   - AAPCS64    (aarch64 Linux/Android):  `va_list = [__va_list; 1]`
// Everywhere else — Windows, arm64 macOS (Apple's ABI uses `char*`),
// 32-bit Unix — `va_list` is already a pointer typedef and the alias is
// fine as-is.
#[cfg(all(unix, target_arch = "x86_64"))]
type VaListArg = *mut mgba_sys::__va_list_tag;
#[cfg(all(any(target_os = "linux", target_os = "android"), target_arch = "aarch64"))]
type VaListArg = *mut mgba_sys::__va_list;
#[cfg(not(any(
    all(unix, target_arch = "x86_64"),
    all(any(target_os = "linux", target_os = "android"), target_arch = "aarch64"),
)))]
type VaListArg = mgba_sys::va_list;

extern "C" {
    fn vsnprintf(
        s: *mut std::os::raw::c_char,
        n: usize,
        format: *const std::os::raw::c_char,
        ap: VaListArg,
    ) -> std::os::raw::c_int;
}

unsafe extern "C" fn c_log(
    _logger: *mut mgba_sys::mLogger,
    category: ::std::os::raw::c_int,
    level: mgba_sys::mLogLevel,
    fmt: *const std::os::raw::c_char,
    args: VaListArg,
) {
    let level = match level {
        mgba_sys::mLogLevel_mLOG_STUB => log::Level::Trace,
        // GAME_ERROR is the ROM doing something weird (divide-by-zero
        // SWIs, misaligned CpuSets, malformed LZ77 streams). Retail
        // BN/EXE games trip these constantly — surfacing them as
        // log::Error spams the console at the default `info` filter.
        // Demote to Debug so they only show under RUST_LOG=debug.
        mgba_sys::mLogLevel_mLOG_DEBUG | mgba_sys::mLogLevel_mLOG_GAME_ERROR => log::Level::Debug,
        mgba_sys::mLogLevel_mLOG_INFO => log::Level::Info,
        mgba_sys::mLogLevel_mLOG_WARN => log::Level::Warn,
        mgba_sys::mLogLevel_mLOG_ERROR | mgba_sys::mLogLevel_mLOG_FATAL => log::Level::Error,
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
            let mut log_filter = Box::new(MaybeUninit::<mgba_sys::mLogFilter>::zeroed());
            mgba_sys::mLogFilterInit(log_filter.as_mut_ptr() as *mut _);
            log_filter.assume_init()
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

/// Install a process-global default logger so any mgba `Core` not driven
/// through `mgba::thread::Thread` (e.g. the prefetch worker's bare core)
/// still routes log lines through the Rust `log` facade instead of
/// falling back to mgba's `printf` stub (which prints unprefixed lines
/// like `GBA BIOS: SWI: 0B r0: …` straight to stdout).
///
/// First call leaks a [`Logger`] and registers it; subsequent calls
/// just re-register the same one. Safe to call from any thread and
/// before any [`mgba::core::Core`] is constructed.
pub fn install_default_logger() {
    static INSTALLED: std::sync::OnceLock<&'static Logger> = std::sync::OnceLock::new();
    let logger = INSTALLED.get_or_init(|| Box::leak(Box::new(Logger::new())));
    unsafe {
        mgba_sys::mLogSetDefaultLogger(logger.as_mlogger_ptr() as *mut _);
    }
}
