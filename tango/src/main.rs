#![windows_subsystem = "windows"]

mod audio;
mod config;
mod discord;
mod game;
mod i18n;
mod input;
mod navicust;
mod net;
mod netplay;
mod patch;
mod pvp_session;
mod randomcode;
mod replay_session;
mod replays;
mod rom;
mod rom_overrides;
mod save;
mod save_view;
mod scanner;
mod scrubber;
mod selection;
mod session;
mod singleplayer_session;
mod stats;
mod tabs;
mod updater;
mod video;
mod widgets;

mod app;
mod theme;

use app::App;

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
///   1. Make sure the logs dir exists; open a timestamped log
///      file inside it.
///   2. Spawn `current_exe()` again with `TANGO_CHILD=1` +
///      `RUST_BACKTRACE=1`, redirecting the child's stderr into
///      the log file so panics + datachannel/mgba C-side stderr
///      get captured.
///   3. Wait. On non-zero exit, pop up a localized rfd dialog
///      pointing at the log file path.
///
/// Returns the exit code we should propagate to the OS.
fn supervisor_main() -> anyhow::Result<i32> {
    use std::io::Write;
    let config = config::Config::load_or_create();
    let lang = config.language.clone();

    let logs_dir = config.logs_path();
    let _ = std::fs::create_dir_all(&logs_dir);
    let ts = chrono::Local::now().format("%Y%m%d%H%M%S").to_string();
    let log_path = logs_dir.join(format!("{ts}.log"));

    let mut log_file = match std::fs::File::create(&log_path) {
        Ok(f) => f,
        Err(e) => {
            rfd::MessageDialog::new()
                .set_title(i18n::t(&lang, "window-title"))
                .set_description(i18n::t_args(
                    &lang,
                    "crash-no-log",
                    &[("error", format!("{e:?}").into())],
                ))
                .set_level(rfd::MessageLevel::Error)
                .show();
            return Err(e.into());
        }
    };

    let exe = std::env::current_exe()?;
    let status = std::process::Command::new(exe)
        .args(std::env::args_os().skip(1).collect::<Vec<std::ffi::OsString>>())
        .env(TANGO_CHILD_ENV_VAR, "1")
        .env("RUST_BACKTRACE", "1")
        .stderr(log_file.try_clone()?)
        .spawn()?
        .wait()?;

    writeln!(&mut log_file, "exit status: {status:?}")?;

    if !status.success() {
        rfd::MessageDialog::new()
            .set_title(i18n::t(&lang, "window-title"))
            .set_description(i18n::t_args(
                &lang,
                "crash",
                &[("path", log_path.display().to_string().into())],
            ))
            .set_level(rfd::MessageLevel::Error)
            .show();
    }

    Ok(status.code().unwrap_or(0))
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

    // Re-parse the CLI in the child (the supervisor doesn't pass
    // it through). Bad args here would have failed in the
    // supervisor already, so unwrap is fine.
    let args = <Args as clap::Parser>::parse();
    let init_link_code = args.command.and_then(|c| match c {
        Command::Join { link_code } => Some(link_code),
    });
    let _ = INIT_LINK_CODE.set(init_link_code);
    // Route mgba's global default logger through `c_log` too — without
    // this, the prefetcher's bare Core falls through to mgba's printf
    // stub and spams `GBA BIOS: SWI: …` lines straight to stdout.
    mgba::log::install_default_logger();

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
    // `vsync: false` cuts the present queue. With vsync iced 0.14
    // pipes frames through wgpu's vsync-locked surface, which on
    // 60 Hz monitors adds a full frame (~16 ms) of presentation
    // latency on top of the emulator's own 1-frame input delay.
    // Without vsync the emulator's freshly rendered frame paints
    // immediately, dropping the perceived input lag from ~3 frames
    // to ~1. Risks light tearing on a 60 Hz monitor; the screen
    // area being moved (the GBA screen) is mostly static so this
    // is barely visible in practice.
    let settings = iced::Settings {
        // Same constant the typographic scale + every markdown
        // Settings::with_text_size call uses, so the body text
        // size is in one place.
        default_text_size: iced::Pixels(app::TEXT_BODY),
        vsync: false,
        ..iced::Settings::default()
    };
    iced::application(App::new, App::update, App::view)
        .settings(settings)
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .window(iced::window::Settings {
            // Initial size; min_size keeps the user from
            // shrinking the window so small the tab strip /
            // sidebars start visually collapsing on top of one
            // another.
            size: iced::Size::new(1000.0, 640.0),
            min_size: Some(iced::Size::new(800.0, 600.0)),
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
        .default_font(iced::Font::with_name("Noto Sans"))
        .run()
}
