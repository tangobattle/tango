//! Browser frontend for single-player, netplay, patches, and replays.
//!
//! The host drives shared sessions through [`engine`], sends audio to an
//! AudioWorklet through [`audio`], and persists the library in IndexedDB
//! through [`storage`]. [`input`] combines touch, keyboard, and gamepads.
//! [`link`] coordinates the lobby; [`playback`], [`recording`], and [`export`]
//! handle replay playback, recording, and video export.
//!
//! See the crate README for browser build requirements and module ownership.

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
        // Where this app's wasm-bindgen glue is served from — build.sh
        // fixes the name, and it sits beside index.html. An engine that
        // spawns Web Workers (the DS games') bootstraps them from this;
        // it cannot reliably discover it from inside the module.
        if let Some(window) = web_sys::window() {
            if let Ok(url) =
                web_sys::Url::new_with_base("tango-lite-web.js", &window.location().href().unwrap_or_default())
            {
                tango_match::hosting::set_wasm_glue_url(url.href());
            }
        }
        input::install_keyboard();
        input::install_gamepads();
        dioxus::launch(app::App);
    }
}
