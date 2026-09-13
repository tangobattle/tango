# Tango

Tango provides rollback netplay for Mega Man Battle Network, with desktop
and browser frontends, save editors, and replay playback and video export.

## Build and run

Install Rust stable, a C/C++ compiler, CMake, Ninja, and Protocol Buffers
(`protoc`). Native dependencies include emulator and networking libraries
built from source. Platform package lists and release setup are in
[the native CI workflow](.github/workflows/ci.yaml).

```sh
cargo run --release --bin tango
```

The desktop app includes BN5 and BN6 as bundled Luau packages. The other games still
use native support, enabled by default. To restrict those native games, disable
the defaults explicitly:

```sh
cargo run --release --bin tango --no-default-features --features gamesupport-bn4
```

Use `cargo build --release --bin tango` to build without launching.
Platform packaging scripts in `linux/`, `macos/`, and `win/` use the
optimized `release-dist` profile. The browser has a separate toolchain
and server requirements; see [its build instructions](tango-lite-web/README.md).

## Find your way around

The [scripted package platform](tango-script/README.md) is under development.
It uses upstream Luau with mlua and strict typing for game-owned editors, ROM transformations, and telemetry,
with `.tangopkg` packages intended to replace Tango patches. The Packages tab
installs local archives with dependency checking and manages installed versions. BN5 and BN6 use packages
for save editing, save creation, single-player launch, and online play; their Rust
game, data-view, and editor crates have been removed. The remaining games keep
native support until their packages are implemented. Unpatched legacy BN5/BN6 replays import through their packages; legacy patches still
need import support.

| Area | Location | Responsibility |
| --- | --- | --- |
| Desktop app | `tango` | Application state, tabs, native input/audio/video, updates |
| Browser app | `tango-lite-web` | Browser UI, canvas, audio worklet, IndexedDB |
| Library | `tango-library` | Game registry, ROM/save/patch/replay scanning, shared settings |
| Scripted packages | `tango-script`, `packages` | Strict Luau runtime, extension packages, script-owned editors and patches |
| Sessions | `tango-session` | Single-player, netplay, training, replay drivers and transport |
| Match engine | `tango-match` | Backend interfaces, rollback coordination, audio, telemetry |
| Emulators | `tango-backend-mgba`, `tango-backend-melonds` | GBA and DS implementations |
| Matchmaking | `tango-lobby`, `tango-net-protocol` | Lobby state and wire messages |
| Replays | `tango-replay`, `tango-replay-renderer` | Recording format and video export |
| Game interface | `tango-gamesupport` | ROM identity, save/editor contracts, engine hooks |
| Per-game support | `tango-gamesupport-<game>` | Registration and game-specific engine integration |
| Save and ROM data | `tango-gamesupport-<game>-dataview` | Binary layouts, parsing, assets |
| Save editors | `tango-gamesupport-<game>-ui` | Per-game editor presentation |
| Shared game support | `tango-gamesupport-common*` | Telemetry, parsing, editor state and controls |
| Shared UI | `tango-ui` | Styles, widgets, animation, copy feedback |

Per-game crates join the workspace through path dependencies. A plain
root build uses `default-members`, excluding the browser-only target.
Use explicit packages when checking portable code; `--workspace` also
selects the browser, which cannot compile for a native target.

[CONTRIBUTING.md](CONTRIBUTING.md) documents checks, module ownership, and
how to add a game or dependency.

## License

GPL-3.0-or-later — see [LICENSE](LICENSE) and [CREDITS.md](CREDITS.md).
Tango links GPL-licensed [melonDS](https://melonds.kuribo64.net/).
The game-support crates in this repository use the same license.
