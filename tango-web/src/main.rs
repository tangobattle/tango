//! tango-web — Tango, the Mega Man Battle Network netplay client, in
//! the browser.
//!
//! A Dioxus port of the desktop client sharing its engine crates (mgba,
//! mgba-siolink, tango-pvp, the per-game gamesupport crates) and — as
//! the port progresses — its wire protocol, for web ↔ desktop
//! crossplay. The web platform layer (audio worklet, WebGL presenter,
//! OPFS storage, runtime pump) follows gbaroll's proven techniques.
//!
//! This crate builds for wasm32 only (`dx serve` / `dx build`).

#[cfg(not(target_arch = "wasm32"))]
compile_error!("tango-web is browser-only: build with `dx serve` (wasm32-unknown-unknown)");

mod config;
mod i18n;
mod library;
mod net;
mod netplay;
mod patches;
mod platform;
mod runtime;
mod session;
mod storage;
mod ui;
mod web;

fn main() {
    web::main();
}
