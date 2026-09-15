# Architecture

Tango has two hosts over the same library, lobby, session drivers, and
match engine. The desktop owns threads and native devices. The browser
pumps drivers from its event loop and supplies browser storage and audio.

## Dependencies

Arrows point from a consumer to the layer it uses. The frontends also use
the replay renderer, which takes an engine replay configuration and emits
video through `encoder-facade`.

```mermaid
flowchart TD
    Desktop[tango: desktop host] --> Library[tango-library]
    Browser[tango-lite-web: browser host] --> Library
    Desktop --> Lobby[tango-lobby]
    Browser --> Lobby
    Desktop --> Session[tango-session]
    Browser --> Session
    Lobby --> Library
    Lobby --> Session
    Session --> Match[tango-match]
    Session --> Protocol[tango-net-protocol]
    Session --> Replay[tango-replay]
    Library --> Games[per-game registrations]
    Games --> Backends[mgba / melonDS backends]
    Backends --> Match
```

| Layer | Owns | Does not own |
| --- | --- | --- |
| Frontends | Screens, input mapping, devices, scheduling, application effects | Rollback or game-specific binary layouts |
| Library | Game registry, ROM/save/patch/replay catalogs, settings, input resolution | Emulation and session lifetime |
| Lobby | Connection negotiation, readiness, committed settings/save exchange | Driving an active match |
| Session | Portable drivers, audio streams, netplay supervision, session controls | UI toolkit, native output devices, desktop worker threads |
| Match | Backend contracts, rollback coordination, telemetry, replay seeking and analysis | Network connections, frontend pacing, storage paths |
| Backend | Console emulation and linked-console operations | Application UI and library scans |
| Per-game support | Registration and game-specific engine hooks | Frontend orchestration |

`tango-gamesupport-<game>-dataview` owns save/ROM layouts and assets;
`-ui` owns editor presentation. These remain separate because the browser
and headless consumers need parsing and emulation without the desktop UI.

## Game registration and features

`tango-library/src/game.rs` contains the one registration list. Each entry
names its core families and editor. `FAMILIES`, `GAMES`, lookup, localization,
and optional editor lookup derive from that list.

`Game` and `Family` are independent of UI features. The library's `ui`
feature enables each selected game's editor through Cargo's weak dependency
forwarding (`dependency?/ui`). Enabling UI alone adds no games. Desktop and
browser `gamesupport-*` flags both forward to the library; the desktop also
enables its `ui` feature.

## Desktop message flow

`main.rs` connects iced to `app::App`. `app/message.rs` defines the messages,
and `app/dispatch.rs` routes them and updates screen transitions. A tab
updates its local UI state and returns an effect when an action needs
application resources. The corresponding app feature module performs it.

| Task | Implementation |
| --- | --- |
| Selection, save operations, single-player/training launch | `app/play.rs` |
| Playback, queue, statistics, export | `app/replay.rs` |
| Downloads and patch tab effects | `app/patches.rs` |
| Scan completion and selected-save reconstruction | `app/library.rs` |
| Lobby settings, compatibility, PvP handoff | `app/lobby.rs` |
| Session actions affecting preferences or the library | `app/sessions.rs` |
| Settings and welcome screen effects | `app/settings.rs` |
| Clipboard, file manager, window events, Discord | `app/desktop.rs` |

The app coordinates these features because they share the selected save,
library, configuration, and one active session. Feature modules implement
methods on that owner; they do not introduce service objects or a second
event bus.

## Session ownership

1. A launcher resolves ROMs, saves, and patches, constructs a portable
   session, and starts its desktop workers.
2. It returns a `Launch` containing the runtime and its presentation data.
   While an asynchronous PvP launch is pending, the lobby stays visible.
3. `State::install` closes the old runtime, resets transient UI state,
   connects audio, and gives the new frame subscription a fresh identity.
4. `RunningSession` owns the session, audio, save backup, and worker handles.
   On drop it requests close, flushes the save, releases audio, drops the
   session to cancel drivers, and joins the workers.

That same teardown handles an abandoned lobby handoff, an error after only
some replay workers started, replacement, explicit close, and application
state destruction. A pending launch never binds the output device.
Post-match results survive session close so watching their replay can return
to the results screen.

The browser owns its scheduling in `engine.rs`. It uses the same portable
session drivers; desktop thread ownership stays in the desktop crate.

## Netplay

`tango-lobby` negotiates settings and committed save data, then yields
`PreMatchData`. `tango-session::pvp::setup` builds a session from it.

- `pvp/driver.rs` boots the pair, advances rollback, publishes frames,
  records confirmed inputs, and finalizes the recording.
- `pvp/supervisor.rs` receives network input, watches the connection,
  reconnects, and announces orderly shutdown.
- `pvp/recording.rs` builds metadata and opens the host's replay store.
- `pvp/mod.rs` exposes controls, status, and shared state.

The host must call `Drive::finish` when a driver ends, so recordings receive
their final marker. The desktop's runtime loop and browser pump do this.
Simulation details and invariants are in
[the match-engine guide](tango-match/ARCHITECTURE.md).

## Replay preparation

A replay stores inputs, so viewing, analyzing, or exporting it runs the game
again. Every consumer uses `tango-library::replays::resolve_roms` to:

1. Resolve both recorded seats against the game registry.
2. Check each recorded simulation version against its backend.
3. Load the clean scanned ROM and apply the exact recorded patch version.
4. Return games and ROM bytes in absolute player order.

The recording's local-player index changes perspective, never seat order.
Missing patches and incompatible simulations fail before emulation starts.
`rom::load` is also used for ordinary session launches. It releases the
scanner lock before patch I/O and leaves the clean cached ROM unchanged.

Storage and HTTP go through the library traits. Native adapters use the
filesystem and reqwest; the browser supplies an IndexedDB-backed memory
image and fetch. Save-editor previews can intentionally recover from a
missing patch; simulation input preparation cannot.
