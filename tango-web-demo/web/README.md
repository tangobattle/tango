# tango-session in a browser

The smallest host that runs a Tango session in a browser: a canvas, a
keyboard map, an AudioWorklet, and a pump. Everything emulator-shaped is
`tango-session`, unchanged from the desktop — this exists to prove the
sessions really are drivable by an event loop rather than a thread.

## Build

Cross-compiling the mgba core needs a wasm-aware clang; see the env in
`~/.cargo/config.toml` (`WASI_SDK_PATH`, `CC_wasm32_unknown_unknown`,
`AR_wasm32_unknown_unknown`, `LIBCLANG_PATH`).

```sh
cargo build -p tango-web-demo --target wasm32-unknown-unknown --release
wasm-bindgen target/wasm32-unknown-unknown/release/tango_web_demo.wasm \
    --out-dir tango-web-demo/web/pkg --target web
```

## Run

```sh
python3 -m http.server -d tango-web-demo/web 8080
```

Open <http://localhost:8080>, pick a ROM (and optionally a `.sav`), and
press Start — the AudioContext needs that gesture. Buttons are
<kbd>←↑↓→</kbd>, <kbd>Z</kbd>/<kbd>X</kbd> for A/B, <kbd>A</kbd>/<kbd>S</kbd>
for L/R, <kbd>Enter</kbd>/<kbd>Shift</kbd> for start/select. "Download
save" pulls the cartridge's savedata back out of the running session.

`?rom=<url>&save=<url>` loads from the server instead of the pickers,
which is handy when iterating on one ROM. `window.demoStats()` in the
console reports what the pump is doing: audio state, sink queue depth,
emulated frames run, and the savedata size.

`pkg/` and `local/` are build output and scratch, and aren't checked in.
