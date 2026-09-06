# Tango Lite

The browser frontend supports single-player, live netplay, patch
installation, and replay recording, playback, and video export. Game
support uses the same ten `gamesupport-*` feature flags as the desktop.
There are no games enabled by default; the build script enables all of them.

## Build and serve

Install Rust nightly with `rust-src` and the `wasm32-unknown-unknown`
target, CMake, Ninja, `protoc`, wasi-sdk, and a wasm-aware libclang.
Install the `wasm-bindgen` CLI version matching `Cargo.lock`.
`wasm-opt` is optional; the deployment workflow uses Binaryen 131.
See [the web workflow](../.github/workflows/web.yaml) for the complete
Linux toolchain setup.

From this directory:

```sh
rustup toolchain install nightly --component rust-src --target wasm32-unknown-unknown
export WASI_SDK_PATH=/path/to/wasi-sdk
export CC_wasm32_unknown_unknown="$WASI_SDK_PATH/bin/clang"
export AR_wasm32_unknown_unknown="$WASI_SDK_PATH/bin/llvm-ar"
# On macOS with Homebrew LLVM, for bindgen:
export LIBCLANG_PATH="$(brew --prefix llvm)/lib"
./build.sh
python3 serve.py 8080
```

Open `http://127.0.0.1:8080`. The DS backend requires shared WebAssembly
memory, so the server must send `Cross-Origin-Opener-Policy: same-origin`
and `Cross-Origin-Embedder-Policy: require-corp`. `serve.py` sends these
locally; `assets/_headers` supplies them on Cloudflare Pages. A plain
`python3 -m http.server` does not send them and cannot run this build.

`build.sh` produces a self-contained `dist/` with the wasm module, JS
glue, stylesheet, icons, and service worker. Override `FEATURES` to choose
games or `PROFILE` to choose a Cargo profile, for example:

```sh
FEATURES=gamesupport-bn6 PROFILE=dev ./build.sh
```

This crate is a workspace member but not a default member. Native builds
must exclude it; build it through `build.sh`, which supplies the shared
memory flags and rebuilds the standard library.

## Module map

| Module | Responsibility |
| --- | --- |
| `app.rs`, `ui/` | Dioxus shell, screens, touch controls, UI polling |
| `library.rs`, `loadout.rs` | Library state and the selected game/save/patch |
| `storage.rs` | Synchronous memory image persisted to IndexedDB |
| `http.rs` | Shared library HTTP interface implemented with fetch |
| `engine.rs` | Session pumping, screen composition, canvas output |
| `audio.rs` | AudioWorklet output and queue management |
| `input.rs` | Touch, keyboard, and gamepad input |
| `link.rs` | Lobby connection and session handoff |
| `recording.rs`, `playback.rs` | Replay storage and playback setup |
| `export.rs` | Replay video export through the shared renderer |
| `lang.rs`, `wakelock.rs` | Game-name locale and screen wake lock |

The engine, library, and lobby state live in thread-locals. `app.rs`
polls them and updates reactive signals when values change; components
reading this state need a changing revision or prop to avoid stale views.

Storage reads are synchronous because patch application and session
construction read through `tango_library::Storage`. IndexedDB writes
mirror the memory image asynchronously. Recording buffers bytes and
persists them when the writer is dropped, including abandoned matches.

Audio is pushed to an AudioWorklet. Animation frames, worker timers, and
audio queue reports all drive the session pump, which uses elapsed time
to avoid advancing twice. The worker timer keeps netplay moving when a
hidden tab stops receiving animation frames. DS emulation also uses
workers, which is why the build requires shared memory.

Desktop features without a browser counterpart include the save editor,
results/stats sidecars, Discord presence, and signaling-free direct UDP
connections. The app UI is not localized; game names use the shared
localization data. Reconnect needs further testing on lossy mobile links.
