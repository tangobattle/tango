//! Parent half of crash capture: the supervisor process that owns the
//! log file, spawns the UI as a child, runs the out-of-process
//! [`minidumper`] dump server for it, and reports a non-zero exit.

use crate::t;

/// How many rotated copies of each log-dir file to keep, i.e.
/// `tango.log` plus `tango.log.1` ..= `tango.log.{MAX_ROTATED_LOGS}`.
const MAX_ROTATED_LOGS: usize = 5;

/// Shift `base` → `base.1` → `base.2` → …, dropping the oldest copy,
/// so `base` is free for this session. The crash dump rotates through
/// the same scheme as the log, so `tango.dmp.N` (if that session
/// crashed) always pairs with `tango.log.N`.
fn rotate_logs(dir: &std::path::Path, base: &str) {
    let numbered = |n: usize| {
        if n == 0 {
            dir.join(base)
        } else {
            dir.join(format!("{base}.{n}"))
        }
    };
    let _ = std::fs::remove_file(numbered(MAX_ROTATED_LOGS));
    for n in (0..MAX_ROTATED_LOGS).rev() {
        let _ = std::fs::rename(numbered(n), numbered(n + 1));
    }
}

/// Parent half of crash handling:
///   1. Make sure the logs dir exists; rotate the previous
///      sessions' logs and open a fresh `tango.log` inside it.
///   2. Spawn `current_exe()` again with `TANGO_CHILD=1` +
///      `RUST_BACKTRACE=1`, redirecting the child's stderr into
///      the log file so panics + datachannel/mgba C-side stderr
///      get captured.
///   3. Wait. On non-zero exit, pop up a localized rfd dialog
///      pointing at the log file path.
///
/// Returns the exit code we should propagate to the OS.
pub fn run() -> anyhow::Result<i32> {
    use std::io::Write;
    let config = crate::config::Config::load_or_create();
    let lang = config.language.clone();

    let logs_dir = config.logs_path();
    let _ = std::fs::create_dir_all(&logs_dir);
    rotate_logs(&logs_dir, "tango.log");
    rotate_logs(&logs_dir, "tango.dmp");
    let log_path = logs_dir.join("tango.log");
    let dump_path = logs_dir.join("tango.dmp");

    let mut log_file = match std::fs::File::create(&log_path) {
        Ok(f) => f,
        Err(e) => {
            rfd::MessageDialog::new()
                .set_title(t!(&lang, "window-title"))
                .set_description(t!(&lang, "crash-no-log", error = format!("{e:?}")))
                .set_level(rfd::MessageLevel::Error)
                .show();
            return Err(e.into());
        }
    };

    // Route the supervisor's own logging into the log file. The minidump
    // server runs here (not in the child), so without this its `log::*`
    // diagnostics — "captured minidump", "failed to send ack", etc. —
    // would vanish (only the child process installs a logger otherwise).
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .target(env_logger::Target::Pipe(Box::new(log_file.try_clone()?)))
        .try_init();

    // Start the out-of-process crash dump server before spawning the
    // child, so the child's connect can't race the bind. If it fails to
    // start we still run — the child just won't get minidumps.
    let sock_path = crash_socket_path();
    let _ = std::fs::remove_file(&sock_path); // clear any stale socket
    let crash_server = start_crash_server(&sock_path, log_file.try_clone()?, dump_path.clone());

    let exe = std::env::current_exe()?;
    let mut cmd = std::process::Command::new(exe);
    cmd.args(std::env::args_os().skip(1).collect::<Vec<std::ffi::OsString>>())
        .env(super::CHILD_ENV_VAR, "1")
        .env("RUST_BACKTRACE", "1")
        .stderr(log_file.try_clone()?);
    if crash_server.is_some() {
        cmd.env(super::CRASH_SOCKET_ENV_VAR, &sock_path);
    }
    let status = cmd.spawn()?.wait()?;

    writeln!(&mut log_file, "exit status: {status:?}")?;

    // Tear down the crash server and remove its socket.
    if let Some((handle, shutdown)) = crash_server {
        shutdown.store(true, std::sync::atomic::Ordering::SeqCst);
        let _ = handle.join();
    }
    let _ = std::fs::remove_file(&sock_path);

    if !status.success() {
        rfd::MessageDialog::new()
            .set_title(t!(&lang, "window-title"))
            .set_description(t!(&lang, "crash", path = log_path.display().to_string()))
            .set_level(rfd::MessageLevel::Error)
            .show();
    }

    Ok(status.code().unwrap_or(0))
}

/// Absolute AF_UNIX socket path for the crash IPC channel. Per-pid so
/// concurrent instances don't collide.
fn crash_socket_path() -> std::path::PathBuf {
    let name = format!("tango-crash-{}.sock", std::process::id());
    // macOS's `$TMPDIR` is a long per-user path (`/var/folders/…/T/`), but
    // this path doubles as the mach port service name and the AF_UNIX
    // `sun_path`, which caps at ~104 bytes — so use short, always-present
    // `/tmp` there. Elsewhere `temp_dir()` is short and correct.
    #[cfg(target_os = "macos")]
    {
        std::path::PathBuf::from("/tmp").join(name)
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::env::temp_dir().join(name)
    }
}

/// Bind the `minidumper` dump server and run it on a background thread.
/// Returns the join handle + shutdown flag, or `None` if it couldn't be
/// started (the child then simply gets no minidumps). The server writes
/// the `.dmp` out-of-process by reading the suspended child.
fn start_crash_server(
    sock_path: &std::path::Path,
    log: std::fs::File,
    dump_path: std::path::PathBuf,
) -> Option<(
    std::thread::JoinHandle<()>,
    std::sync::Arc<std::sync::atomic::AtomicBool>,
)> {
    let mut server = match minidumper::Server::with_name(sock_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("could not start crash dump server: {e:?}");
            return None;
        }
    };
    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let shutdown_thread = shutdown.clone();
    // A separate handle so a fatal `run()` error (e.g. `UnknownClientPid`
    // or a mach recv failure — which propagate out rather than reaching
    // `on_minidump_created`) still lands in the log instead of vanishing.
    let mut err_log = log.try_clone().ok();
    let handler = CrashServerHandler {
        log: std::sync::Mutex::new(log),
        dump_path,
        sock_path: sock_path.to_path_buf(),
    };
    let handle = std::thread::spawn(move || {
        if let Err(e) = server.run(Box::new(handler), &shutdown_thread, None) {
            if let Some(l) = err_log.as_mut() {
                use std::io::Write;
                let _ = writeln!(l, "crash dump server error: {e:?}");
                let _ = l.flush();
            }
        }
    });
    Some((handle, shutdown))
}

/// `minidumper` server-side hooks: pick where the dump goes, and record
/// what happened into the same log file the child's stderr streams into.
struct CrashServerHandler {
    log: std::sync::Mutex<std::fs::File>,
    dump_path: std::path::PathBuf,
    sock_path: std::path::PathBuf,
}

impl minidumper::ServerHandler for CrashServerHandler {
    fn create_minidump_file(&self) -> Result<(std::fs::File, std::path::PathBuf), std::io::Error> {
        let file = std::fs::File::create(&self.dump_path)?;
        Ok((file, self.dump_path.clone()))
    }

    fn on_minidump_created(
        &self,
        result: Result<minidumper::MinidumpBinary, minidumper::Error>,
    ) -> minidumper::LoopAction {
        use std::io::Write;
        if let Ok(mut log) = self.log.lock() {
            // The child does no logging in its handler (see `client.rs`), so
            // the whole crash block is written here — one writer, so it can't
            // interleave out of order.
            let _ = writeln!(log, "\n=== native crash ===");
            match result {
                Ok(md) => {
                    let _ = writeln!(log, "minidump written: {}", md.path.display());
                }
                Err(e) => {
                    let _ = writeln!(log, "minidump FAILED: {e:?}");
                }
            }
            let _ = writeln!(log, "=== end native crash ===\n");
            let _ = log.flush();
        }
        minidumper::LoopAction::Continue
    }

    fn on_message(&self, _kind: u32, _buffer: Vec<u8>) {
        // The child's handler only calls `request_dump` — it never sends a
        // user message — so there's nothing to handle here.
    }

    fn on_client_connected(&self, _num_clients: usize) -> minidumper::LoopAction {
        // The child is connected: the endpoint keeps working off our open
        // fds, so unlink the socket name now. Nothing can hijack it and
        // there's nothing left to clean up. (Best-effort — deleting a bound
        // AF_UNIX file may fail on Windows; the post-run remove covers that.)
        let _ = std::fs::remove_file(&self.sock_path);
        minidumper::LoopAction::Continue
    }

    fn on_client_disconnected(&self, _num_clients: usize) -> minidumper::LoopAction {
        // The (single) child has gone; stop the loop so the supervisor
        // can join this thread promptly.
        minidumper::LoopAction::Exit
    }
}
