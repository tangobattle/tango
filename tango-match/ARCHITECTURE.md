# Match engine

`tango-match` coordinates deterministic simulation over emulator-independent
interfaces. GBA games use `tango-backend-mgba`; DS games use
`tango-backend-melonds`. A game's registration supplies a `Backend`, so
sessions and replay consumers use the same engine API for either console.

The crate owns no network connection, UI, audio device, or drive thread.
The host advances the simulation and decides how to pace it. See
[the workspace architecture](../ARCHITECTURE.md) for the surrounding layers.

## Interfaces

| Type | Responsibility |
| --- | --- |
| `Backend` | Game-specific entry point: rates, layout, simulation version, solo/match/replay construction |
| `Link` | Two linked consoles advanced, captured, and restored as one unit |
| `Side` | Borrowed view of one console's display, audio, save data, and telemetry |
| `Console` | One independently running console |
| `HostInput` | Buttons and optional stylus coordinates |
| `Snapshot` | Opaque capture understood by its originating backend |

Virtual dispatch happens at console/frame operations. Game-specific offsets,
priming traps, and RAM interpretation remain in the game support and backend
crates.

## Live simulation

`engine::Match` wraps a `getgud` rollback session over a `Link`. Both peers
simulate the same pair, in absolute player order. Each advance supplies local
input, predicts missing remote input, and re-simulates from a snapshot when a
prediction changes. `Link::sanitize` normalizes inputs to the console's
actual controls before simulation and transmission.

The host receives confirmed input pairs and telemetry. The boundary
`[0, confirmed)` identifies settled ticks; replay writing and statistics
consume only that settled history. `Throttler` turns clock skew and
speculation balance into a pacing adjustment for the leading peer.

`solo::Solo` drives one console for single-player sessions. Training uses a
local linked match with a host-supplied dummy controller.

## Audio and display

The simulation pushes audio into `audio::channel`. A host consumes the ring
without locking the emulator. Snapshot bookkeeping records audio publication
marks: rollback revokes queued speculative sound, and sound already consumed
becomes a debt that regenerated audio pays down.

The backend declares a `ScreenLayout` and supplies RGBA8 frames matching it.
The host selects visible seats and screens, allowing replay PiP, training
views, and DS layout preferences without teaching the simulation about UI
widgets. Sessions compose multiple screens side by side;
`tango-session::screens` re-arranges that composition for presentation. Replay/video consumers keep the backend's rational frame and sample
rates exact until pacing or resampling requires floating point.

## Replay and analysis

The recording format lives in **`tango-replay`**, not this crate. It stores
boot metadata, SRAM, RNG seed, and confirmed input pairs. The library resolves
recorded game identities, simulation versions, and exact patches before any
re-simulation begins.

`replay::ReplaySet` is constructed through the backend's `ReplayBoot`:

- `Playback` advances recorded inputs over a linked pair.
- `Capture` includes simulation state and frames for seek previews.
- `SnapshotStore` holds sparse keyframes; `RewindRing` retains recent frames.
- `Replay` combines playback with seek operations.
- `StatsPass` re-simulates for keyframes, round marks, and optional statistics.
- `SeekController` lets a newer seek supersede work on an older target.

`tango-session::replay` exposes the work as driven workers. The desktop gives
playback, seeking, and prefetching their own threads. The browser uses a
combined driver that budgets the work across event-loop turns.
`tango-session::replay::EngineReplay` is the one mapping from a decoded
recording to the local seat's backend and its `ReplayConfig`; playback,
headless analysis (`tango-session::replay::analyze`), and both hosts' video
export build from it.

`analysis::StatsBuilder` folds confirmed samples and events for live matches
and offline replay analysis. `telemetry` stores rollback-aware observations;
`battle` defines their data. The stats codec is here; filesystem sidecar paths
and persistence live in `tango-library::stats`.

## Determinism contracts

- Both peers use identical ROMs, saves, RNG seed, match settings, and RTC in
  identical seat order. The RTC is the negotiated match clock, also recorded
  for playback.
- Priming reaches a real linked battle through the games' own protocol. Game
  hooks automate setup; they do not replace the emulated link.
- Backend snapshots include both consoles and the state of their connection.
  Restoring a snapshot must reproduce the same future for the same inputs.
- Speculative telemetry is truncated on rollback. Confirmed inputs, stats,
  and match events refer to the settled timeline.
- Audio output and presentation choices must not alter simulation state.
- `Backend::sim_version` changes when the same inputs would produce a
  different match. Lobby compatibility and replay loading compare the same
  opaque value. Wire-format changes use the protocol version; recording-layout
  changes use `tango_replay::VERSION`.

Construction and advancement return `Result`. Priming is bounded and
cancellable. The session layer turns failures into status and shutdown; a
frontend decides how to present them.

## Verification

The crate has ROM-free tests for rollback/input ordering, audio, statistics,
and replay infrastructure. Run `cargo test --locked -p tango-match`.
Workspace and frontend checks are listed in [CONTRIBUTING.md](../CONTRIBUTING.md).

Automated checks do not replace real-game validation for changes to backend
emulation or game hooks: play a match through round and match end, seek a
recording across round boundaries, and compare replay statistics and export.
