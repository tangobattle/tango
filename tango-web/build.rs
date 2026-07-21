//! Compiles the audio worklet's DSP module (../tango-web-worklet) so
//! `platform::audio::web` can embed the bytes and ship them into the
//! AudioWorkletGlobalScope, which can't fetch. It's a separate tiny
//! wasm module because the main binary's wasm-bindgen output can't run
//! in a worklet scope (no DOM, no text codecs) — and a separate cargo
//! invocation with its own target dir so the two builds can't deadlock
//! on a lock or bleed flags into each other.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=../tango-web-worklet/src");
    println!("cargo:rerun-if-changed=../tango-web-worklet/Cargo.toml");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target_dir = out_dir.join("worklet-target");
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(&cargo)
        .args(["build", "--release", "--target", "wasm32-unknown-unknown"])
        .current_dir("../tango-web-worklet")
        .env("CARGO_TARGET_DIR", &target_dir)
        // The parent build's flags are for the parent build.
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .status()
        .expect("couldn't spawn cargo for tango-web-worklet");
    assert!(status.success(), "tango-web-worklet build failed");

    let wasm = target_dir.join("wasm32-unknown-unknown/release/tango_web_worklet.wasm");
    std::fs::copy(&wasm, out_dir.join("tango_web_worklet.wasm"))
        .expect("couldn't copy worklet wasm");
}
