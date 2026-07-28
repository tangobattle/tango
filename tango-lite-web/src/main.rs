//! Tango Lite — a phone-sized Tango that runs in a browser tab.
//!
//! Load a ROM, play it, or dial a link code and play someone. The
//! emulator, the netplay choreography and the rollback engine are the
//! workspace's own crates, unmodified: [`tango_session`] was written so
//! that a session is *driven* by its host rather than running itself,
//! and [`tango_lobby`] so that bringing a connection up is one linear
//! future and the lobby is a plain state machine. What this crate adds
//! is only the browser end of those contracts:
//!
//! * [`engine`] — the pump. `requestAnimationFrame` instead of a drive
//!   thread, and a 2D canvas instead of a wgpu pane.
//! * [`audio`] — a `ScriptProcessorNode` pulling the same
//!   [`CoreStream`](tango_session::audio::CoreStream) cpal pulls on the
//!   desktop.
//! * [`input`] — touch buttons and a keyboard map, resolved into one
//!   GBA joyflag word.
//! * [`link`] — the netplay state machine, pumped from the microtask
//!   queue and handed off into a live match.
//! * [`storage`] — the ROM/save locker, in IndexedDB.
//!
//! Deliberately *not* here: the ROM scanner, patches, replays, the save
//! editor, the results screen. Lite means the two things you'd open a
//! phone for — play, and play someone.

// The whole app is browser code. Gated as a unit so a host-target build
// reports this one line rather than a page of errors from crates that
// have no browser to bind to.
#[cfg(not(target_arch = "wasm32"))]
compile_error!(
    "tango-lite-web is a browser app: build it for wasm32-unknown-unknown (see build.sh). \
     It is a workspace member but not a default member, so a plain root `cargo build` skips it."
);

#[cfg(target_arch = "wasm32")]
mod app;
#[cfg(target_arch = "wasm32")]
mod audio;
#[cfg(target_arch = "wasm32")]
mod engine;
#[cfg(target_arch = "wasm32")]
mod export;
#[cfg(target_arch = "wasm32")]
mod http;
#[cfg(target_arch = "wasm32")]
mod input;
#[cfg(target_arch = "wasm32")]
mod lang;
#[cfg(target_arch = "wasm32")]
mod library;
#[cfg(target_arch = "wasm32")]
mod link;
#[cfg(target_arch = "wasm32")]
mod loadout;
#[cfg(target_arch = "wasm32")]
mod playback;
#[cfg(target_arch = "wasm32")]
mod recording;
#[cfg(target_arch = "wasm32")]
mod storage;
#[cfg(target_arch = "wasm32")]
mod ui;
#[cfg(target_arch = "wasm32")]
mod wakelock;

fn main() {
    #[cfg(target_arch = "wasm32")]
    {
        // Panics reach the console as a stack trace instead of
        // "unreachable executed", which is the difference between a
        // debuggable browser build and an opaque one.
        console_error_panic_hook::set_once();
        let _ = console_log::init_with_level(log::Level::Info);
        input::install_keyboard();
        input::install_gamepads();
        dioxus::launch(app::App);
    }
}
