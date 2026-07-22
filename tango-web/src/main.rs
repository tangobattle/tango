//! tango-web — Tango, the Mega Man Battle Network netplay client, in
//! the browser and (via dioxus-native) on the desktop.
//!
//! A Dioxus port of the desktop client sharing its engine crates (mgba,
//! mgba-siolink, tango-pvp, the per-game gamesupport crates) and — as
//! the port progresses — its wire protocol, for web ↔ desktop
//! crossplay. The web platform layer (audio worklet, WebGL presenter,
//! OPFS storage, runtime pump) follows gbaroll's proven techniques.
//!
//! Two builds from one crate:
//! - wasm32: `dx serve` / `dx build`, dioxus-web in a browser.
//! - native: plain `cargo run`, dioxus-native (Blitz — no webview),
//!   with the platform layer filled in by the desktop client's stack
//!   (SDL3 audio + gamepads, wgpu custom paint, real directories,
//!   libdatachannel + tokio-tungstenite, ffmpeg export).

mod analysis;
mod compat;
mod config;
mod export;
mod host;
mod i18n;
mod library;
mod net;
mod netplay;
mod patches;
mod platform;
mod rom_overrides;
mod runtime;
mod save_view;
mod session;
mod storage;
mod ui;

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;

fn main() {
    #[cfg(target_arch = "wasm32")]
    web::main();
    #[cfg(not(target_arch = "wasm32"))]
    native::main();
}
