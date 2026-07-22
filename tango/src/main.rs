#![windows_subsystem = "windows"]

// Foundations.
mod config;
mod i18n;
mod library; // the on-disk game library: registry + ROM/save/patch/replay scanning
mod platform; // host-machine glue: SDL, input devices, AV output, crash capture
mod ui; // look-and-feel toolkit: widgets, style, theme, animation

// Netplay: `net` (in the session crate) owns the wire protocols,
// `netplay` the connection lifecycle.
pub(crate) use tango_session::net;
mod netplay;

// App-level state the tabs share.
mod loadout;
mod save_edit;
mod selection;

// Screens.
mod save_view;
mod session;
mod tabs;

// The replays tab's ffmpeg encode pipeline (runs on its own thread).
mod replay_export;

// Side services, and the app shell that ties everything together.
mod app;
mod discord;
mod updater;

use app::App;

use crate::tabs::settings::MINIMUM_RESOLUTION;

// Bundled fonts. We reuse the main app's font files (a few MB total)
// so JP / SC / TC scripts render instead of tofuing out, and so the
// monospace chip-code badge matches the rest of the UI. cosmic-text
// automatically falls back to whichever registered font has the
// requested glyph when the default doesn't.
const FONT_NOTO_SANS: &[u8] = include_bytes!("../fonts/NotoSans-Regular.ttf");
const FONT_NOTO_SANS_JP: &[u8] = include_bytes!("../fonts/NotoSansJP-Regular.otf");
const FONT_NOTO_SANS_SC: &[u8] = include_bytes!("../fonts/NotoSansSC-Regular.otf");
const FONT_NOTO_SANS_TC: &[u8] = include_bytes!("../fonts/NotoSansTC-Regular.otf");
const FONT_NOTO_SANS_MONO: &[u8] = include_bytes!("../fonts/NotoSansMono-Regular.ttf");
const FONT_NOTO_EMOJI: &[u8] = include_bytes!("../fonts/NotoEmoji-Regular.ttf");
// Lucide icon font ships with the `lucide-icons` crate as
// `LUCIDE_FONT_BYTES`; registered with iced below.

/// Set by the parent supervisor when it spawns the child UI
/// process. Presence of this env var (set to `"1"`) tells
/// `main` to skip the supervisor branch and just run the iced
/// app.
const TANGO_CHILD_ENV_VAR: &str = "TANGO_CHILD";

/// Set by the supervisor to the `minidumper` IPC socket path the child
/// connects to for out-of-process crash dumps. Absent when the child is
/// launched directly (then native crashes just get a stderr note).
const TANGO_CRASH_SOCKET_ENV_VAR: &str = "TANGO_CRASH_SOCKET";

/// CLI shape — matches legacy `tango/src/main.rs::Args` so
/// Discord deep-links and the `tango Join <code>` command-line
/// invocation behave the same way.
#[derive(clap::Parser, Debug, Clone)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(clap::Subcommand, Debug, Clone)]
enum Command {
    /// Jump straight to the Play tab with the given netplay link
    /// code pre-filled. Used by `tango://join/<code>` style URI
    /// handlers + Discord "Join Game" intents.
    Join { link_code: String },
}

pub fn main() {
    if std::env::var(TANGO_CHILD_ENV_VAR).as_deref() == Ok("1") {
        // Child process — the actual UI. Stderr is captured by
        // the parent into the log file, so any panic backtrace
        // (with RUST_BACKTRACE=1 set by the parent) lands there.
        if let Err(e) = run_app() {
            eprintln!("iced app exited with error: {e:?}");
            std::process::exit(1);
        }
        return;
    }
    // Parse CLI in the supervisor so `--help` / bad args fail
    // fast without spawning a child. The parsed value is
    // re-derived in the child via `std::env::args` so we don't
    // have to serialize it through the supervisor boundary.
    let _args = <Args as clap::Parser>::parse();
    // Parent / supervisor — set up the log file, spawn the
    // child, and surface an rfd dialog on non-zero child exit.
    match supervisor_main() {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("crash supervisor failed: {e:?}");
            std::process::exit(2);
        }
    }
}

/// Parent half of the crash-handling trampoline. Mirrors
/// `tango/src/main.rs`'s parent flow:
///   1. Make sure the logs dir exists; rotate the previous
///      sessions' logs and open a fresh `tango.log` inside it.
///   2. Spawn `current_exe()` again with `TANGO_CHILD=1` +
///      `RUST_BACKTRACE=1`, redirecting the child's stderr into
///      the log file so panics + datachannel/mgba C-side stderr
///      get captured.
///   3. Wait. On non-zero exit, pop up a localized rfd dialog
///      pointing at the log file path.
///
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

/// Returns the exit code we should propagate to the OS.
fn supervisor_main() -> anyhow::Result<i32> {
    use std::io::Write;
    let config = config::Config::load_or_create();
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
                .set_title(i18n::t!(&lang, "window-title"))
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
        .env(TANGO_CHILD_ENV_VAR, "1")
        .env("RUST_BACKTRACE", "1")
        .stderr(log_file.try_clone()?);
    if crash_server.is_some() {
        cmd.env(TANGO_CRASH_SOCKET_ENV_VAR, &sock_path);
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
            .set_title(i18n::t!(&lang, "window-title"))
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
            // The child does no logging in its handler (see crash_log.rs), so
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

/// Initial link code parsed from CLI args, stashed in a global
/// so `App::new` (which iced calls with no arguments) can pick
/// it up. Set once at startup; cleared after the first read so
/// re-runs don't replay the same code.
static INIT_LINK_CODE: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

/// Decode `icon.png` into an iced `window::Icon`. Returns
/// `None` on any failure (image-crate decode error, dimension
/// mismatch, etc.) — the OS just falls back to its default
/// icon, no need to escalate.
fn load_window_icon() -> Option<iced::window::Icon> {
    let img = image::load_from_memory(include_bytes!("icon.png")).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    iced::window::icon::from_rgba(img.into_raw(), w, h).ok()
}

fn run_app() -> iced::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // Catch native crashes (segfaults, SEH violations, Mach
    // EXC_BAD_ACCESS) from mgba / datachannel / wgpu C code. Connect to
    // the supervisor's out-of-process dump server (if we were launched
    // by one) so the minidump is written from the healthy parent rather
    // than in this process's fault handler. Also installs a panic hook
    // for Rust panics. Leak the handle so it stays installed for the
    // lifetime of the process.
    let crash_client =
        std::env::var_os(TANGO_CRASH_SOCKET_ENV_VAR).and_then(|name| {
            match minidumper::Client::with_name(std::path::Path::new(&name)) {
                Ok(c) => Some(c),
                Err(e) => {
                    log::error!("could not connect to crash dump server: {e:?}");
                    None
                }
            }
        });
    std::mem::forget(platform::crash_log::install(crash_client));

    // Re-parse the CLI in the child (the supervisor doesn't pass
    // it through). Bad args here would have failed in the
    // supervisor already, so unwrap is fine.
    let args = <Args as clap::Parser>::parse();
    let init_link_code = args.command.map(|c| match c {
        Command::Join { link_code } => link_code,
    });
    let _ = INIT_LINK_CODE.set(init_link_code);
    // Route mgba's global default logger through `c_log` too — without
    // this, the prefetcher's bare Core falls through to mgba's printf
    // stub and spams `GBA BIOS: SWI: …` lines straight to stdout.
    mgba::log::install_default_logger();

    // Initialize SDL3 itself + warm the gamepad context now (main
    // thread) so the first emulator session's first redraw doesn't
    // pay for SDL_Init + enumerating + opening every attached
    // controller, and so the audio backend below can borrow the
    // already-initialized Sdl. sdl3 enforces "first thread to call
    // init owns the pump", so this has to happen on the iced/winit
    // main thread.
    platform::sdl_init::init();
    platform::gamepad::init();

    // Windows-only auto-fallback to ANGLE for old Intel iGPUs.
    // We enumerate `Backends::PRIMARY` (DX12 + Vulkan) up front
    // with a throwaway `wgpu::Instance`; if no adapter shows up,
    // set `WGPU_BACKEND=gl` so iced's wgpu compositor picks the
    // GL backend instead, which dlopens the bundled `libEGL.dll`
    // (ANGLE) and translates GLES → D3D11. `enumerate_adapters`
    // is synchronous and wraps per-backend init internally — a
    // broken Vulkan driver yields an empty list rather than a
    // crash — so this is a real check, not a heuristic.
    //
    // The `WGPU_BACKEND` env var still wins if the user set it
    // (e.g. `WGPU_BACKEND=dx12` to force the native path for
    // diagnostics).
    #[cfg(windows)]
    if std::env::var_os("WGPU_BACKEND").is_none() {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });
        let adapters = instance.enumerate_adapters(wgpu::Backends::PRIMARY);
        if adapters.is_empty() {
            log::warn!(
                "no DX12 / Vulkan adapter available — auto-falling back to ANGLE (set WGPU_BACKEND=dx12 or =vulkan to override)"
            );
            // SAFETY: still single-threaded here — no tokio
            // runtime, no iced compositor — so no other thread
            // can read env concurrently.
            std::env::set_var("WGPU_BACKEND", "gl");
        } else {
            let names: Vec<_> = adapters
                .iter()
                .map(|a| {
                    let info = a.get_info();
                    format!("{:?} {}", info.backend, info.name)
                })
                .collect();
            log::info!("primary adapter(s) detected: {names:?}");
        }
    }

    // Body text default. Every text widget that doesn't pass an
    // explicit `.size(...)` picks this up — that's the bulk of the
    // UI. Iced's bare default is 16 px; 13 matches what the rest
    // of the typographic scale (TEXT_TITLE / TEXT_HEADING /
    // TEXT_CAPTION) was tuned against.
    //
    // `vsync: false` so `iced_wgpu` picks `AutoNoVsync` →
    // `PresentMode::Immediate` on every backend that supports
    // it (DX12, Vulkan, Metal). On Metal that flips
    // `CAMetalLayer.displaySyncEnabled = false`
    // (wgpu-hal/metal/surface.rs:150) so the Fifo swap-chain
    // queue collapses; the macOS WindowServer still composites
    // at the panel's refresh, but we save the ~1 frame of
    // present-queue depth that `Fifo` insists on. On the GBA's
    // mostly-static framebuffer the residual tearing risk on
    // Linux/Win is barely perceptible.
    let settings = iced::Settings {
        // Same constant the typographic scale + every markdown
        // Settings::with_text_size call uses, so the body text
        // size is in one place.
        default_text_size: iced::Pixels(ui::style::TEXT_BODY),
        vsync: false,
        ..iced::Settings::default()
    };
    // Load config once here just for window geometry. App::new
    // reloads it for the rest of its state (cheap — JSON parse).
    // This double-load keeps the window-size restore self-contained
    // in main without threading a Config handle into App::new.
    let geom_cfg = config::Config::load_or_create();
    let (min_w, min_h) = MINIMUM_RESOLUTION;
    let (start_w, start_h) = geom_cfg.last_window_size.unwrap_or((min_w as f32, min_h as f32));
    // Only restore the position for a fullscreen relaunch, where the
    // saved value is the last fullscreen monitor's origin — this keeps
    // a fullscreen Tango on its monitor across launches. Windowed
    // launches let the OS place the window (centered), since restoring
    // an exact x/y is janky on multi-monitor setups.
    let start_position = match (geom_cfg.fullscreen, geom_cfg.last_window_position) {
        (true, Some((x, y))) => iced::window::Position::Specific(iced::Point::new(x, y)),
        _ => iced::window::Position::default(),
    };

    iced::application(App::new, App::update, App::view)
        .settings(settings)
        .title(App::title)
        .theme(App::theme)
        .scale_factor(App::scale_factor)
        .subscription(App::subscription)
        .window(iced::window::Settings {
            // min_size keeps the user from shrinking the window so
            // small the tab strip / sidebars start visually
            // collapsing on top of one another. Initial size +
            // maximized state come from the last shutdown.
            size: iced::Size::new(start_w, start_h),
            min_size: Some(iced::Size::new(min_w as f32, min_h as f32)),
            position: start_position,
            maximized: geom_cfg.last_window_maximized,
            fullscreen: geom_cfg.fullscreen,
            // OS-level window icon (title bar + taskbar). Same
            // PNG we render in the nav strip; iced wants raw
            // RGBA so decode once at startup. Best-effort —
            // a corrupt asset just leaves the OS default icon.
            icon: load_window_icon(),
            ..iced::window::Settings::default()
        })
        .font(FONT_NOTO_SANS)
        .font(FONT_NOTO_SANS_JP)
        .font(FONT_NOTO_SANS_SC)
        .font(FONT_NOTO_SANS_TC)
        .font(FONT_NOTO_SANS_MONO)
        .font(FONT_NOTO_EMOJI)
        .font(lucide_icons::LUCIDE_FONT_BYTES)
        // iced 0.14's cosmic-text falls back across registered
        // faces, so we can default to the Latin Noto Sans and let
        // CJK / emoji glyphs come from the JP / SC / TC / Emoji
        // fonts above.
        .default_font(ui::style::DEFAULT_FONT)
        .run()
}
