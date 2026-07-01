//! Crash logging. Installs a Rust panic hook and a native crash
//! handler ([`crash_handler::CrashHandler`]) so segfaults / SEH
//! exceptions / mach EXC_BAD_ACCESS — i.e. crashes coming from
//! mgba, datachannel, or wgpu / driver code — are captured.
//!
//! Native crashes are handled **out-of-process**: on a fault the
//! child's handler does the bare minimum — hand the crash context to
//! the supervisor over the [`minidumper`] IPC channel — and the
//! supervisor (see `main.rs`) writes a minidump by reading the
//! suspended child. This is the Breakpad/Crashpad model: all the
//! heavy, not-signal-safe work (walking memory, writing the dump)
//! happens in the healthy parent, not in the crashed child. Load the
//! resulting `.dmp` in WinDbg / lldb / `minidump-stackwalk` with the
//! debug info kept from that build.
//!
//! Rust panics (not hardware faults) still unwind normally, so the
//! panic hook keeps capturing a symbolicated backtrace to stderr,
//! which the supervisor pipes into the timestamped log file.

use std::backtrace::Backtrace;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

/// Guards against re-entrant crashes (e.g. a second fault raised
/// while we're handling the first). Without this we'd loop.
static IN_HANDLER: AtomicBool = AtomicBool::new(false);

/// Install the panic hook and the native crash handler. `client` is
/// the connected [`minidumper::Client`] the handler uses to ask the
/// supervisor for a dump; `None` (e.g. the child was launched without
/// a supervisor) degrades to a stderr note. Returns the
/// [`crash_handler::CrashHandler`] so the caller can keep it alive for
/// the process's lifetime (dropping it uninstalls).
pub fn install(client: Option<minidumper::Client>) -> crash_handler::CrashHandler {
    std::panic::set_hook(Box::new(|info| {
        let bt = Backtrace::force_capture();
        let mut stderr = std::io::stderr().lock();
        let _ = writeln!(stderr, "\n=== panic ===\n{info}\n{bt}\n=== end panic ===\n");
        let _ = stderr.flush();
    }));

    // SAFETY: `make_crash_event` is unsafe because the closure runs in
    // a signal / exception context. Ours only does a single IPC send on
    // the already-open `minidumper` socket (plus, on failure, a stderr
    // note) — the dump itself is written by the supervisor.
    let handler = crash_handler::CrashHandler::attach(unsafe {
        crash_handler::make_crash_event(move |cc: &crash_handler::CrashContext| {
            if IN_HANDLER.swap(true, Ordering::SeqCst) {
                // Re-entrant crash — get out of the way and let the OS
                // finish us off.
                return crash_handler::CrashEventResult::Handled(false);
            }
            match &client {
                // Do the absolute minimum in the handler: a single IPC send on
                // minidumper's private socket/port, blocking until the
                // supervisor has written the minidump from our (suspended)
                // memory. The supervisor writes the crash block to the log
                // (see `on_minidump_created` in main.rs), so we ignore the
                // return value — its ack is best-effort and spuriously errors
                // on macOS *after* a successful dump.
                //
                // Deliberately NO stderr / allocation on this path: on macOS
                // the handler runs on the mach-exception thread while the
                // faulting thread is suspended, and the app logs to stderr
                // constantly — grabbing the stderr lock (or the malloc lock)
                // that the suspended thread might hold would deadlock the
                // handler, and we'd get no dump at all.
                Some(client) => {
                    let _ = client.request_dump(cc);
                }
                // No supervisor (child launched directly, e.g. in dev): nothing
                // will write a dump, so fall back to a stderr note.
                None => {
                    let mut stderr = std::io::stderr().lock();
                    let _ = writeln!(stderr, "\n=== native crash ===\n{}\n(no crash server; minidump not written)\n=== end native crash ===\n", describe(cc));
                    let _ = stderr.flush();
                }
            }
            // `Handled(false)` = let the crash propagate so the OS still
            // kills the process with the right signal / exit code. The
            // supervisor sees a non-zero exit and pops up the "open log
            // file" dialog.
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
