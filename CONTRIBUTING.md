# Working on Tango

## Checks

Run these from the repository root. Python 3.11+ is needed only for the
workspace convention check. Use the checked-in `Cargo.lock`; avoid
updating dependencies as part of unrelated changes.

```sh
python3 tools/check_workspace.py
cargo fmt --all -- --check
cargo check --locked --bin tango --all-features
```

The portable core can be tested without native library adapters or a ROM:

```sh
cargo test --locked --no-default-features \
  -p tango-library -p tango-match -p tango-net-protocol \
  -p tango-replay -p tango-gamesupport-common-dataview
```

Exercise the real filesystem/HTTP patch adapter, a registered game, the
session drivers, and save editor with separate commands:

```sh
cargo test --locked --lib -p tango-library \
  --features tango-library/gamesupport-bn6 \
  -p tango-session
cargo test --locked --lib -p tango-gamesupport-common-ui
```

Keep these commands separate: Cargo unifies features across the selected
packages. The editor enables `tango-gamesupport/ui`, which requires any
game aggregator in the same build to enable its own `ui` feature too.
The native library check intentionally exercises headless BN6 support.

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

Keep game-specific offsets, save formats, and ROM assets in the game's
`-dataview` crate. Keep emulator hooks and registrations in its aggregator
crate, and editor presentation in its `-ui` crate. The aggregator's `ui`
feature is the boundary that keeps headless consumers free of UI code.

`tango-match` defines backend-independent simulation contracts.
`tango-session` exposes drivers advanced by the host; desktop threads
and browser event loops supply the pacing. Session code uses
`tango-session::platform` for waits that must also work in a browser.

The engine interfaces report tick and audio sample rates as
`num_rational::Ratio<u32>`. Keep those ratios exact through replay export;
use `num_traits::ToPrimitive` when pacing or resampling needs a float.

`tango-library` accesses storage and HTTP through its `Storage` and
`Http` traits. Their native adapters are behind `native`; the browser
supplies an IndexedDB-backed memory image and fetch. Shared code must
not bypass these adapters with direct filesystem or networking calls.

Within the larger modules:

- `tango/src/app/mod.rs` owns state, initialization, and subscriptions.
  `message.rs` defines messages and destinations, `dispatch.rs` routes
  incoming messages, `update.rs` handles tab effects, and `view.rs`
  renders the shell.
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
`gamesupport-all` list. The desktop additionally forwards the game's
`ui` feature. `tools/check_workspace.py` catches divergent game lists,
repeated dependency versions, and missing workspace lint inheritance.

Run `cargo fmt --all` after Rust edits. Keep comments about current
behavior and constraints beside the code; keep setup and architecture
explanations in the READMEs. ROM files are local inputs and are ignored
by Git.
