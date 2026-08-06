//! What a browser host tells the engines about the page it runs on.
//!
//! An engine that spawns worker threads has to hand each Web Worker
//! the URL of the app's wasm-bindgen glue script — the one thing about
//! the page an engine cannot reliably discover from inside the module
//! (stack-trace tricks read a `wasm://` frame or a framework snippet as
//! often as the real thing). The host knows its own file layout, so it
//! states the URL here once at startup, and an engine that needs it
//! reads it in [`Backend::prepare`](crate::Backend::prepare).
//!
//! wasm32-only: nowhere else does a thread need help spawning.

use std::sync::Mutex;

static GLUE_URL: Mutex<Option<String>> = Mutex::new(None);

/// State where the app's wasm-bindgen glue script is served from.
/// Called once by the host before any session is put together.
pub fn set_wasm_glue_url(url: String) {
    *GLUE_URL.lock().unwrap() = Some(url);
}

/// The host's stated glue URL, for an engine spawning workers.
pub fn wasm_glue_url() -> Option<String> {
    GLUE_URL.lock().unwrap().clone()
}
