# Working on Tango

## Checks

Use release mode for Cargo builds, checks, tests and runs.
Run these from the repository root. Python 3.11+ is needed only for the
workspace convention check. Use the checked-in `Cargo.lock`; avoid
updating dependencies as part of unrelated changes.

```sh
python3 tools/check_workspace.py
cargo fmt --all -- --check
cargo check --release --locked --bin tango --all-features
```

The portable core can be tested without native library adapters or a ROM:

```sh
cargo test --release --locked --no-default-features \
  -p tango-library -p tango-match -p tango-net-protocol \
  -p tango-replay -p tango-gamesupport-common-dataview -p tango-script
```

Exercise the real filesystem/HTTP patch adapter, a registered game, the
session drivers, and save editor with separate commands:

```sh
cargo test --release --locked --lib -p tango-library \
  --features tango-library/gamesupport-bn4 \
  -p tango-session
cargo test --release --locked --lib -p tango-gamesupport-common-ui -p tango-script-iced
cargo test --release --locked -p tango-script-iced --test presentation
```

Keep these commands separate: Cargo unifies features across the selected
packages. The editor enables `tango-gamesupport/ui`, which requires any
game aggregator in the same build to enable its own `ui` feature too.
The native library check intentionally exercises headless BN4 support.
The presentation tests compare pixels and input events against native Tango
widgets using Iced's software renderer; they do not need a window server. Set
`TANGO_UI_CAPTURE_DIR` to retain both sets of PNGs for visual inspection. These
primitive checks do not replace a game's complete embedded-editor parity gate.

For wider native coverage, including every game, use:

```sh
cargo test --release --locked --workspace --exclude tango-lite-web --all-features --lib
cargo test --release --locked --bin tango --all-features
```

Patch integration tests start a local HTTP server and use temporary
directories. Only tests that require BN4's registry entries are gated by
`gamesupport-bn4`; native adapter tests are gated by `native`. The browser
build is checked separately using its documented nightly toolchain.

The `checks` workflow runs formatting, workspace conventions, an unused
dependency audit (`cargo machete`), and the portable/native test suites for pull requests and pushes to `main`.
The `ci` workflow builds platform checks on `main` to warm release caches.
`release` packages native tags; `web` builds and deploys the browser app.

## Ownership and boundaries

The destination is a game-neutral Rust core. New package-based game behavior,
including offsets, save formats, ROM assets, editor controls, training rules,
and patches, belongs in strict Luau under `packages/`; use upstream Luau through
`tango-script`. Do not add Battle Network-specific knowledge to the package
host. See [the package contract and migration steps](tango-script/README.md).
Use `snake_case` for Luau functions, variables, and table fields, `PascalCase`
for types, and `SCREAMING_SNAKE_CASE` for constants. Keep Luau built-ins and
external format names unchanged. Collection positions are 1-based, including
model slots, selections, reorder events, and row styles. Convert native indices
at the Rust/Luau or binary-format boundary; byte offsets, coordinates, and encoded
game IDs keep their original values. Use `nil` for an absent optional selection.
Imports use the standard global `require`.
Use `@package` for declared dependencies and `./` or `../` for modules inside
the same package. Keep `packages/.luaurc` in sync when adding package folders;
stock Luau LSP uses its aliases with the root `.vscode/settings.json` or
`.zed/settings.json` for editor checking. Tango checks dependency permissions
and exact versions separately.
Extend a game through dependent packages that wrap or replace existing Luau
methods. ROM changes belong in `patch_rom`; editor changes belong in ordinary
method replacements. Do not add a separate game-specific override data schema.
Packages may independently export `[[editor]]`, `[[gamemode]]`, and
`[[telemetry]]` modules. Every path resolves inside its package; `init.luau`
remains the ordinary import and package-test entry. Use `--editor NAME` or
`--gamemode NAME` to select among multiple exports. Reserve “ruleset” for
folder constraints. Training scripting is deferred.
Declare each selectable match type as its own gamemode. Its `setup(context,
game)` registers address callbacks with `game.hook`; callbacks read/write the
emulator through `game` and report lifecycle changes directly. Keep changing
game state in emulator memory/registers, which rollback already captures.
Captured setup constants are recreated for each callback; there is no separate
gamemode state buffer.
Packages exposing gamemodes must declare `default_gamemode` in `package.toml`.
Use `context.disable_bgm` for the host's BGM setting; package-specific options
live in `context.options`. Native games also declare flat choices and their
own default; mode/subtype pairs exist only at the old replay format boundary.
The embedded save editor must retain its capabilities and familiar layout;
visual differences are acceptable when they simplify the implementation. Follow
[the migration checks](tango-script/EDITOR_PARITY.md) before replacing any
existing game's editor. The basic package development window is not that UI.

Until migration is complete, existing native support keeps its current
boundaries: data in `-dataview`, hooks/registrations in the aggregator, and
presentation in `-ui`. The aggregator's `ui` feature keeps headless consumers
free of UI code. Remove each game’s native crates, registrations, and feature wiring as its
package takes over; do not keep two implementations. BN5 and BN6 have migrated.
Preserve unrelated local edits when retiring a crate.
Once these native implementations are removed, fold app-facing game/editor
orchestration into `tango`; do not preserve a separate `tango-gamesupport` layer
solely for dispatch. Shared emulator/rollback contracts remain in their portable
crates, and Luau execution remains in `tango-script`.

`tango-match` defines backend-independent simulation contracts.
`tango-session` exposes drivers advanced by the host; desktop threads
and browser event loops supply the pacing. Session code uses
`tango-session::platform` for waits that must also work in a browser.
Sessions take a `SessionBackend` and prepared cartridge images, never native
game registrations. Keep save conversion, legacy replay mode lookup and
optional game artwork in the host.

The engine interfaces report tick and audio sample rates as
`num_rational::Ratio<u32>`. Keep those ratios exact through replay export;
use `num_traits::ToPrimitive` when pacing or resampling needs a float.

`tango-library` accesses storage and HTTP through its `Storage` and
`Http` traits. Their native adapters are behind `native`; the browser
supplies an IndexedDB-backed memory image and fetch. Shared code must
not bypass these adapters with direct filesystem or networking calls.

Within the larger modules:

- `tango/src/package/` owns the host's package capabilities: `editor.rs`
  and its submodules embed editors and provide development commands;
  `catalog.rs` discovers ROM capabilities and resolves simulations;
  `gamemode.rs` owns named selections; `replay.rs` resolves recorded
  configurations and normalizes explicit legacy imports; `selection.rs` caches disk-save recognition
  and loading; `telemetry.rs` presents scripted telemetry.

- `tango/src/app/mod.rs` owns state, initialization, and subscriptions.
  `message.rs` defines messages and destinations, `dispatch.rs` routes
  incoming messages, `update.rs` handles tab effects, and `view.rs`
  renders the shell. `packages.rs` runs package-management effects off-thread;
  `tabs/packages/` owns the inventory screen.
- `tango-library/src/patch/mod.rs` exposes the patch API and ROM patching.
  `catalog.rs` merges and scans metadata; `download.rs` fetches and
  validates legacy patches. Keep its entry points usable during migration;
  the replacement package path is `tango-library/src/package.rs`. Its
  `catalog.rs` inventories every source and resolves exports; `management.rs`
  supplies the dependency-aware installer/remover used by the app and CLI.
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

When maintaining a legacy native game, register its families in
`tango-library/src/game.rs` and add the corresponding `gamesupport-*`
feature to the library and both frontends. Include it in each
`gamesupport-all` list. The desktop additionally forwards the game's
`ui` feature. `tools/check_workspace.py` catches divergent game lists,
repeated dependency versions, and missing workspace lint inheritance.

Run `cargo fmt --all` after Rust edits. Keep comments about current
behavior and constraints beside the code; keep setup and architecture
explanations in the READMEs. ROM files are local inputs and are ignored
by Git.
