# Working on Tango

## Checks

Run these from the repository root. Python 3.11+ is needed only for the
workspace convention check. Use the checked-in `Cargo.lock`; avoid
updating dependencies as part of unrelated changes.

```sh
python3 tools/check_workspace.py --resolved
cargo fmt --all -- --check
cargo check --locked --bin tango --all-features
```

The portable core can be tested without native library adapters or a ROM:

```sh
cargo test --locked --no-default-features \
  -p tango-library -p tango-match -p tango-net-protocol \
  -p tango-replay -p tango-gamesupport-common-dataview
cargo test --locked --no-default-features --lib -p tango-session
cargo test --locked --lib -p tango-lobby -p tango-net
```

Exercise the real filesystem/HTTP patch adapter, a registered game, the
session drivers, and save editor together:

```sh
cargo test --locked --lib -p tango-library \
  --features tango-library/gamesupport-bn6 \
  -p tango-session -p tango-gamesupport-common-ui
cargo test --locked --lib -p tango-library \
  --features tango-library/ui,tango-library/gamesupport-bn6
```

The first command deliberately combines headless BN6 registrations with
an editor consumer. `Game` and `Family` have the same shape regardless of
UI features, so Cargo feature unification must not break this combination.
The second checks the library's optional game-to-editor registration.

For wider native coverage, including every game, use:

```sh
cargo test --locked --workspace --exclude tango-lite-web --all-features --lib
cargo test --locked --bin tango --all-features
```

Patch integration tests start a local HTTP server and use temporary
directories. Only tests that require BN6's registry entries are gated by
`gamesupport-bn6`; native adapter tests are gated by `native`. The browser
build is checked separately using its documented nightly toolchain.

The `checks` workflow runs formatting, workspace conventions, an unused
dependency audit (`cargo machete`), and the portable/native test suites for pull requests and pushes to `main`.
The `ci` workflow builds platform checks on `main` to warm release caches.
`release` packages native tags; `web` builds and deploys the browser app.

## Ownership and boundaries

[ARCHITECTURE.md](ARCHITECTURE.md) maps dependencies and the main flows.

Keep game-specific offsets, save formats, and ROM assets in the game's
`-dataview` crate. Keep emulator hooks and registrations in its aggregator
crate, and editor presentation in its `-ui` crate. The library registry
connects families to editors through its optional `ui` feature. Its weak
feature forwarding enables editors only for games already selected.

`tango-match` defines backend-independent simulation contracts.
`tango-session` exposes drivers advanced by the host; desktop threads
and browser event loops supply the pacing. Session code uses
`tango-platform` (also re-exported as `tango-session::platform`) for waits
that must also work in a browser. Transport lives in `tango-net`; the lobby
must not depend on library catalogs, sessions, or game backends. Offline
sessions must build with `--no-default-features` without transport code.
Hosts supply recording and stats sinks; session code must not write files.

The engine interfaces report tick and audio sample rates as
`num_rational::Ratio<u32>`. Keep those ratios exact through replay export;
use `num_traits::ToPrimitive` when pacing or resampling needs a float.

`tango-library` accesses storage and HTTP through its `Storage` and
`Http` traits. Their native adapters are behind `native`; the browser
supplies an IndexedDB-backed memory image and fetch. Shared code must
not bypass these adapters with direct filesystem or networking calls.

Within the larger modules:

- `tango/src/app/mod.rs` owns state, initialization, and subscriptions.
  `message.rs` defines messages and destinations; `dispatch.rs` routes
  messages and tracks screen transitions. Feature modules (`play`,
  `replay`, `patches`, `settings`, `library`, `lobby`, `sessions`) keep
  handlers with their effects and helpers. `desktop.rs` owns OS actions
  and presence; `view.rs` renders the shell.
- `tango/src/session/launch.rs` constructs a `Launch` from library inputs.
  `State::install` is the only installation path. `runtime.rs` owns audio,
  save persistence, and worker teardown, including abandoned launches.
- `tango-session/src/pvp/` separates the public session controls from
  setup, frame driving, network supervision, and recording.
- `tango-library::loadout::Resolver` prepares and validates exact launch
  inputs for both hosts. `tango-gamesupport-common-dataview::model` owns
  save edits and snapshots; UI code formats its validation findings.
- `tango/src/app/downloads.rs` owns download attempts and cancellation;
  `replay_controller.rs` owns deferred playback and analysis jobs.
- `tango-lite-web/src/host.rs` composes explicit state handles. Core
  browser operations take a handle instead of using global state.
- `tango-library::rom::load` prepares a clean or patched ROM;
  `tango-library::replays::resolve_roms` applies recorded versions and
  checks simulation compatibility for playback, analysis, and export
  in both frontends.
- `tango-library/src/patch/mod.rs` exposes the patch API and ROM patching.
  `catalog.rs` merges and scans metadata; `download.rs` fetches and
  validates packages. The public `patch::*` entry points remain stable.
- `tango-gamesupport-common-ui/src/editor/view/mod.rs` composes the editor
  shell. `state.rs` owns preferences and edit transitions, `actions.rs`
  defines inputs and host effects, and `components.rs` contains reusable
  controls. Per-section modules render the actual game data.

## Dependencies and game features

Declare external dependencies used by multiple crates in the root
`[workspace.dependencies]` table, in alphabetical order. Members inherit
with `dependency.workspace = true`, adding their own features and
`optional = true` where needed. Preserve `default-features` settings:
Cargo adds workspace features to member features, and changing defaults
can accidentally pull native code into a browser build.

When adding a game, register its families in
`tango-library/src/game.rs` and add the corresponding `gamesupport-*`
feature to the library and both frontends. Include it in each
`gamesupport-all` list and add `tango-gamesupport-<game>?/ui` to the library's
`ui` feature. Frontend game features forward only to the library; the
desktop enables `tango-library/ui`. `tools/check_workspace.py` checks
registration, feature forwarding, shared dependencies, workspace lints,
and architectural dependency boundaries.

Run `cargo fmt --all` after Rust edits. Keep comments about current
behavior and constraints beside the code; keep setup and architecture
explanations in the READMEs. ROM files are local inputs and are ignored
by Git.
