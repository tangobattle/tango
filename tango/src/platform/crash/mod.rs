//! Native crash capture, split across two processes: the
//! [`supervisor`] parent that writes logs and minidumps, and the
//! [`client`] hooks installed in the child UI process. The two meet
//! through the environment variables below.

pub mod client;
pub mod supervisor;

/// Set by the parent supervisor when it spawns the child UI
/// process. Presence of this env var (set to `"1"`) tells
/// `main` to skip the supervisor branch and just run the iced
/// app.
const CHILD_ENV_VAR: &str = "TANGO_CHILD";

/// Set by the supervisor to the `minidumper` IPC socket path the child
/// connects to for out-of-process crash dumps. Absent when the child is
/// launched directly (then native crashes just get a stderr note).
const CRASH_SOCKET_ENV_VAR: &str = "TANGO_CRASH_SOCKET";

/// Whether this process is the supervised child UI rather than the
/// supervisor.
pub fn is_child() -> bool {
    std::env::var(CHILD_ENV_VAR).as_deref() == Ok("1")
}
