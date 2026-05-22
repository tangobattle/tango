//! Crash logging. Installs a Rust panic hook and a native crash
//! handler ([`crash_handler::CrashHandler`]) so segfaults / SEH
//! exceptions / mach EXC_BAD_ACCESS — i.e. crashes coming from
//! mgba, datachannel, or wgpu / driver code — land in the same
//! log file as ordinary panics.
//!
//! The supervisor process pipes the child's stderr into the
//! timestamped log file, so writing to stderr here is the same
//! as appending to the log.
//!
//! Async-signal-safety: capturing a full symbolicated
//! [`std::backtrace::Backtrace`] inside a signal handler is not
//! technically signal-safe (it allocates and may call back into
//! the dynamic linker). We do it anyway because we want the full
//! info; the realistic failure mode is a hang inside the
//! allocator if the crash itself happened mid-`malloc`. If that
//! ever bites in practice, downgrade to
//! `backtrace::trace_unsynchronized` and write raw frame
//! addresses.

use std::backtrace::Backtrace;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

/// Guards against re-entrant crashes (e.g. a second SIGSEGV
/// raised while we're formatting the first one's backtrace).
/// Without this we'd loop until the stack overflows.
static IN_HANDLER: AtomicBool = AtomicBool::new(false);

/// Install both the panic hook and the native crash handler.
/// Returns the [`crash_handler::CrashHandler`] so the caller can
/// keep it alive for the process's lifetime (dropping it
/// uninstalls).
pub fn install() -> crash_handler::CrashHandler {
    std::panic::set_hook(Box::new(|info| {
        let bt = Backtrace::force_capture();
        let mut stderr = std::io::stderr().lock();
        let _ = writeln!(stderr, "\n=== panic ===\n{info}\n{bt}\n=== end panic ===\n");
        let _ = stderr.flush();
    }));

    // SAFETY: `make_crash_event` is unsafe because the closure
    // runs in a signal / exception context. Our closure only
    // touches stderr and a backtrace capture — see the
    // module-level note on signal safety.
    let handler = crash_handler::CrashHandler::attach(unsafe {
        crash_handler::make_crash_event(move |cc: &crash_handler::CrashContext| {
            if IN_HANDLER.swap(true, Ordering::SeqCst) {
                // Re-entrant crash — get out of the way and let
                // the OS finish us off.
                return crash_handler::CrashEventResult::Handled(false);
            }
            let bt = Backtrace::force_capture();
            let mut stderr = std::io::stderr().lock();
            let _ = writeln!(
                stderr,
                "\n=== native crash ===\n{}\n{bt}\n=== end native crash ===\n",
                describe(cc),
            );
            let _ = stderr.flush();
            // `Handled(false)` = we logged it, now let the crash
            // propagate so the OS still kills the process with
            // the right signal / exit code. The supervisor sees
            // a non-zero exit and pops up the "open log file"
            // dialog.
            crash_handler::CrashEventResult::Handled(false)
        })
    })
    .expect("attach crash handler");
    handler
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn describe(cc: &crash_handler::CrashContext) -> String {
    // crash-handler stuffs `signalfd_siginfo` here, not the POSIX
    // `siginfo_t`, so the field is `ssi_signo`.
    format!("signal {} (tid {})", cc.siginfo.ssi_signo, cc.tid)
}

#[cfg(target_os = "macos")]
fn describe(cc: &crash_handler::CrashContext) -> String {
    match &cc.exception {
        Some(e) => format!("mach exception kind={} code={} subcode={:?}", e.kind, e.code, e.subcode,),
        None => "mach exception (no info)".to_string(),
    }
}

#[cfg(windows)]
fn describe(cc: &crash_handler::CrashContext) -> String {
    format!(
        "exception code 0x{:08x} (thread {})",
        cc.exception_code as u32, cc.thread_id,
    )
}

// crash-handler's Windows invalid-parameter / pure-virtual fallback
// path calls MSVC's `_invoke_watson`, an undocumented CRT helper
// that triggers Watson reporting. mingw-w64's libmsvcrt.a doesn't
// export it, so the cross-compile to `x86_64-pc-windows-gnu`
// fails with `undefined reference to '_invoke_watson'`. Provide a
// stub that aborts — Watson reporting was already going to kill us,
// abort just skips the dialog. The SEH / vectored handler path
// (what actually catches mgba / datachannel segfaults) doesn't
// touch this.
#[cfg(all(windows, target_env = "gnu"))]
#[no_mangle]
pub extern "C" fn _invoke_watson() -> ! {
    std::process::abort()
}
