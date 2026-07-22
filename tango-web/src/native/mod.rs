//! Native bootstrap and platform glue: dioxus-native (Blitz — no
//! webview) instead of a browser, with the desktop client's stack
//! underneath (SDL3 audio/gamepads, real directories, libdatachannel).
//! The counterpart of `crate::web`; the component tree itself lives in
//! `crate::ui`.

use std::path::PathBuf;

use crate::library;
use crate::runtime::Runtime;

pub fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    mgba::log::install_default_logger();
    // SDL (audio + gamepads only; winit owns the window) must
    // initialize on the main thread, before the event loop takes it.
    crate::platform::sdl_init::init();
    crate::platform::gamepad::init();

    dioxus_native::launch_cfg(crate::ui::App, Vec::new(), vec![Box::new(window_attributes())]);
}

fn window_attributes() -> dioxus_native::WindowAttributes {
    dioxus_native::WindowAttributes::default()
        .with_title("Tango")
        .with_inner_size(dioxus_native::LogicalSize::new(1080.0, 720.0))
}

/// Ensure the audio sink exists, then boot the detected game. The sink
/// is built at `Runtime::install` on native (no user-gesture rule), so
/// the ensure step is a formality kept for API parity.
pub async fn boot(
    runtime: std::rc::Rc<std::cell::RefCell<Runtime>>,
    game: library::GameRef,
    rom: Vec<u8>,
    save: Option<Vec<u8>>,
    save_file: Option<String>,
) -> anyhow::Result<()> {
    ensure_audio(&runtime).await;
    runtime.borrow_mut().start_local(game, rom, save, save_file)
}

/// No-op: the SDL backend is installed at `Runtime::install`; there is
/// no gesture requirement to satisfy.
pub async fn ensure_audio(_runtime: &std::rc::Rc<std::cell::RefCell<Runtime>>) {}

/// No-op: only the browser's `<input type=file>` needs its value
/// cleared between picks.
pub fn reset_file_input(_evt: &dioxus::events::FormEvent) {}

/// "Download" a byte blob the way a desktop app does: write it into the
/// user's Downloads folder, dodging name collisions.
pub fn download_bytes(name: &str, bytes: &[u8]) {
    let dir = directories_next::UserDirs::new()
        .and_then(|d| d.download_dir().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    let mut path = dir.join(name);
    if path.exists() {
        let (stem, ext) = match name.rsplit_once('.') {
            Some((s, e)) => (s.to_owned(), format!(".{e}")),
            None => (name.to_owned(), String::new()),
        };
        for n in 1.. {
            path = dir.join(format!("{stem} ({n}){ext}"));
            if !path.exists() {
                break;
            }
        }
    }
    match std::fs::write(&path, bytes) {
        Ok(()) => log::info!("saved {}", path.display()),
        Err(e) => log::error!("couldn't save {}: {e}", path.display()),
    }
}
