# Scripted game packages

`tango-script` uses **upstream Luau 0.736 through mlua 0.12.1** for compilation
and execution, with native code generation enabled on desktop. In-process
strict type checking uses upstream Luau Analysis and the new solver. The
checker and runtime resolve imports through the same package graph.

The browser frontend retains its `wasm32-unknown-unknown` build and currently
does not embed package editors. `tango-library`'s `packages` feature enables
the C++ scripting dependency and is included in its default `native` feature.
An upstream Luau integration for the existing browser target remains to be
implemented; this change does not introduce Emscripten or a second Luau VM.

Packages are the successor to `.tangopatch`. A game, an editor extension, and a
ROM patch use the same manifest, dependency resolution, container, and runtime.
There is no separate patch metadata schema containing Battle Network rules.

The embedded replacement preserves Tango's save-editor capabilities and familiar
layout. Exact visual matching can give way to a simpler implementation;
[EDITOR_PARITY.md](EDITOR_PARITY.md) defines the reference, functional coverage,
and comparisons required before switching any game over.
The standalone package window is only a development harness.
The normal app embeds package editors in Play, replay previews and in-match
setup panels. Its library combines bundled packages with installed archives
and directory packages under `<data folder>/packages`. Each editor opts into
automatic selection with `detect_rom`; the recognition rules live in Luau.
BN5 and BN6 use package editors, gamemodes, and telemetry. Unported games and their
legacy patches continue through the native registry during migration.
Converting `.tangopatch` archives into dependent packages remains to be implemented.
The [UI architecture](UI_ARCHITECTURE.md) describes the general widget, canvas,
event, model, and embedding capabilities needed for arbitrary games' editors.

`packages/bn5` exports an editor, single/triple gamemodes, and telemetry for US
and Japanese Protoman/Colonel. Its editor includes folders, NaviCust, Patch Cards,
Auto Battle Data, the team Navi picker, and Light/Dark save creation. The native
game, data-view and editor crates have been removed. The normal game/save
pickers, embedded panels, templates and solo boot use the package. Complete-panel
captures and NaviCust pointer/keyboard checks match the native reference, apart
from a small Patch Card text-rendering difference. BN5 live lobby handoff and
recording remain unverified: the local UDP check could not bind a socket in the restricted test environment.
Complete native matches and unpatched legacy simulation-revision-1 replays now
match the package frame by frame, including replay capture restoration.
BN5 and BN6 share round lifecycle
hooks, legacy replay identity checks, unit/hand telemetry, chart presentation, save masking/checksums, ROM table
readers, NaviCust materialization, active-effect selection, build warnings and
editor session controls in `bn-common`. Navigation motion lives in `editor-common`.
Auto Battle Data ranking, filtering, count editing, presentation and text export
are shared for the remaining game ports. Menu flow,
cartridge addresses, effect catalogs and save layouts stay with each game.

`packages/bn4` contains its save codec, folder/NaviCust/Mod Card/Auto Battle Data
models, ROM readers, effect catalogs, derived stats and Mod Card legality. It has
no editor, gamemode or telemetry exports yet, so the existing native game remains
active while those components are ported. The codec handles shifted
saves and checksum-based regional/variant identity; two identity bytes live
only in the decoded document and are removed when writing SRAM. Its native
fixtures cover all twelve templates, 96 shifted/region-ambiguous inputs, and
540 folder mutations and 3,228 additional model mutations. Folder, NaviCust and
Auto Battle Data storage are shared with BN5 through `bn-common/save`; BN4's
six fixed Mod Card slots have their own model. All 266 regional Mod Card entries,
188 NaviCust effect entries and ten bug groups match native references. Local
ROM comparisons cover all four supported cartridges: every chip, element icon
and NaviCust part matches native text, fields and pixels. These checks establish
the data layer; embedded-editor and match parity are still pending.

## What works today

- Load a development directory or a `.tangopkg` archive, check its Luau against
  the host SDK, and resolve an exact-version dependency graph.
- Bundle and install immutable package versions through `tango-library`'s
  `Storage` interface. Reinstalling identical contents is idempotent; replacing
  an installed version with different contents is rejected.
- Render package-defined layouts, labels, images, inputs, choices, buttons,
  checkboxes, sliders, reorderable lists, and canvas display lists through the
  game-neutral `tango-script-iced` crate. Scripts own decoding, encoding, validation, and edits. Rust
  owns transactions, undo/redo, file dialogs, atomic saves, and close handling.
- Run a whole-document Edit → Save/Cancel session. Packages compose its controls;
  the host enforces edit access, cancellation and acknowledgment of the exact write.
- Supply named host actions and inline/external control placement when embedding.
  Packages request enabled actions through transactional `invoke` effects.
- Transform ROM bytes through the selected package's `patch_rom` callback.
  A BPS patch is an ordinary package file, applied with `tango.bps`.
- Export selectable gamemodes with instruction callbacks, direct memory/register
  access and lifecycle signals. The GBA adapter runs them through ordinary
  match/replay interfaces and restores emulator state on rollback.
- Supply named immutable inputs (ROMs and other external data) through bounded
  reads. Documents retain the same inputs through edits, undo/redo, and saves.
- Localize package UI, diagnostics and names with package-owned Fluent catalogs,
  locale fallback, interpolation, plural/select rules and shared library translations.
- Run package-authored Luau tests through the same restricted host.
- Copy package-defined text, self-contained HTML, and raster/drawing images
  through validated host effects, with transient button feedback after success.

The bundled `bn6` package edits Navi selection, the equipped folder and MegaMan's
NaviCust for US/JP Gregar/Falzar saves, plus patch cards for Japanese MegaMan saves.
Its binary layout, masking, checksum,
anticheat mirror, UI, and four save fixtures are all in the package. It preserves
the whole SRAM image and original regional layout. The development editor
uses a matching ROM for its folder library, chip names, icons, and description
tooltips. The package decodes chip attributes, artwork, element icons,
NaviCust parts, Navi emblems, and patch-card effects in Luau. Its save model
supports regular/tag slots, the chip pack, Navi selection, NaviCust placements,
and patch-card mutations; these operations are compared against the native
implementation. Shared Luau folder rules provide class/copy limits, code
availability, Regular/Tag memory checks, and grouped localized warnings. With a
ROM input, BN6 validation also checks patch-card memory and NaviCust
materialization, and the document host blocks saving an invalid build.
The shared folder edit applier handles top insertion, compaction, ordered moves,
Regular/Tag bookkeeping and memory guards. The shared folder UI provides the
two panes, search/sort controls, add/remove/clear actions, drag reorder, and
Regular/Tag controls. The shared patch-card editor provides registered and
available lists, effect badges, search/sort, add/remove/clear, drag reorder, and
memory warnings. It preserves existing disabled cards and allows over-budget
edits while blocking invalid saves, matching the native editor. The NaviCust
editor has the board, part palette and badges, placement ghosts, pickup and
replacement, rotation, compression, filtering, sorting, and clearing. Its
geometry, placement rules, drawing, and controls live in `bn-common/navicust/`;
the BN6 adapter supplies save and ROM data. Local component comparisons cover
selected layouts and edit sequences. Responsive wrapping and picker transitions
deliberately simplify the native presentation.
The shared Navi strip and picker show ROM-derived emblems, colors, names and
build statistics. Selecting a Link Navi closes the picker and hides unavailable
sections while preserving the editor's other state. Cropping, accent selection,
layout and controls live in `bn-common/navi/`; the BN6 adapter uses its scripted
save model and ROM roster. The host supplies general color mixing, shadows,
clipping and text wrapping for this presentation.
Read-only mode supplies the identity strip, grouped/ungrouped folder, NaviCust,
and patch-card viewers, with capability-dependent navigation. The NaviCust
viewer displays the materialization stored in the save, highlights hovered
parts, and shares installed-part badges with the editor. Patch-card rows retain
disabled effects, memory warnings and their native column layout. Folder exports
include grouped/ungrouped TSV and HTML with embedded chip icons. NaviCust exports
include TSV and the stored grid as an image; patch-card TSV omits disabled cards.
The session view offers copy controls while viewing and Edit/Cancel/Save in the
identity strip. The underlying clipboard effects also work in editable documents.
The settled tab strip includes capability-dependent sections, warning badges,
tooltips, horizontal scrolling, edge fades, and per-section copy/grouping controls.
The host can supply Play as enabled, disabled, or absent, and place edit/copy
controls outside the script. Streamer mode shows package-owned regional logos
until Review is selected, including in read-only panels. Entrance motion covers
save arrival, Review, edit-mode changes, tab navigation, and picker open/close or
selection. Picker changes restart a simple entrance; Edit/Save/Cancel controls
switch immediately. The app adapter preserves draft state across failed writes
and rejects stale messages after a save or panel configuration changes.
The [ROM data comparison](ROM_DATA_PARITY.md) records native-decoder coverage and
the optional local reference test format.

The desktop lobby lists compatible package gamemodes in its match-type picker.
BN6 offers Single, Triple, and Random Battle. Both peers resolve the selected
package contents and options before committing, then launch the package backend.
Playback, seeking, analysis, and video export reproduce the recorded package
configurations. The main game picker also lists cartridges recognized by installed
package editors or gamemodes, using the package's localized name and ROM filename.
Select that entry and a save to use the embedded scripted editor. The adjacent
picker selects a named gamemode, including any ROM transformations it supplies.
Save recognition runs the package decoder without rendering the editor or checking
build warnings, so saves needing edits remain selectable. Each cartridge remembers
its save and pinned gamemode. Gamemode-only packages use the exact selected save
file without requiring an editor. Games awaiting migration retain native support
and the legacy patch installer.
Training scripting is deferred.

## Try it

From the repository root (replace `my-save.sav` with a save file):

```sh
cargo run --release --locked --bin tango -- package check packages/gba packages/editor-common packages/bn-common packages/bn6
cargo run --release --locked --bin tango -- package test packages/gba packages/editor-common packages/bn-common packages/bn6
cargo run --release --locked --bin tango -- package edit --save my-save.sav --rom my-rom.gba --gamemode single \
  packages/gba packages/editor-common packages/bn-common packages/bn6
```

Add `--read-only` to open the package's viewer. The host permits view-state
changes, such as grouping, but rejects document mutations and disables saving.
The BN6 viewer shows the identity strip and available folder, NaviCust and
patch-card sections, including their copy controls.

Supply the dependencies and put the selected package last. The development window
opens in view mode. Choose **Edit** to stage changes across every section;
**Cancel** restores the last saved model and **Save** writes all staged edits.
Undo/redo track document transactions while editing;
search and sort selections are view state. Undo and redo reset view state so stale
drafts cannot hide the restored document. Reload recompiles the selected packages
from disk and refuses to discard committed unsaved edits. The host toolbar,
close confirmation, notices and errors use Tango's Fluent catalogs in all eleven
supported languages. `--locale` selects the language for both host controls and
package translations; when opened through Tango, it defaults to the app setting.

As the native implementations are removed, the app-facing game registry and
editor/match orchestration will live in `tango` instead of a separate
`tango-gamesupport` crate. Portable emulator and rollback contracts remain in
`tango-match` and `tango-session`; package execution remains in `tango-script`.
The current `tango-gamesupport` crate stays only while legacy game crates and
the shared desktop/web library still depend on its registrations and parsers.

Bundle and install:

```sh
cargo run --release --locked --bin tango -- package bundle packages/bn6 /tmp/bn6.tangopkg
cargo run --release --locked --bin tango -- package install /tmp/bn6.tangopkg --root /tmp/tango-packages
cargo run --release --locked --bin tango -- package check packages/gba packages/editor-common packages/bn-common /tmp/tango-packages/bn6/0.1.0.tangopkg
```

Open the **Packages** tab in Tango to see bundled packages, installed archives,
development folders, libraries, exported capabilities, and dependency errors.
**Install packages…** accepts multiple `.tangopkg` files together. Selected files
can supply one another's dependencies. The complete graphs are checked against
those files, installed versions, and bundled packages before any archives are
written. Removal protects exact-version dependents and only deletes installed
archives; bundled packages and development directories remain available. The
**Legacy patches** button opens the old installer for games still using native
support.

The CLI uses the same installer and also accepts multiple files or development
directories in one command. Omit `--root` to use Tango's configured packages
directory. The app scans `<root>/<name>/<version>.tangopkg` archives and
flat `<root>/<name>/package.toml` development directories. It resolves exact
dependencies from that snapshot and its bundled libraries. Invalid packages,
missing dependencies and conflicting contents under the same name/version are
reported independently; unrelated exports remain available.

Automatic editor selection tries the newest release of each package's named
editor (or its newest prerelease if no release provides that editor). Exactly
one editor must recognize the ROM. Ambiguous matches are rejected. All versions
remain addressable by an explicit `ExportRef` in the catalog API. The desktop keeps an editor open while its ROM, save contents, and editor package
graph remain unchanged. Changes to those inputs rebuild it; unrelated scan changes
preserve its draft and view state.

The development commands `check`, `test`, `edit`, and `patch-rom` still take an
explicit list of package paths. Remote catalog fetching, automatic dependency
installation, and a package management screen remain to be implemented. The
desktop picker uses installed packages. Selecting a gamemode applies `patch_rom`
and freezes its result into the match and replay identity; its editor receives
that effective ROM. Native games awaiting migration retain the legacy patch path.

Use `cargo run --release --locked --bin tango -- package <command>` for package
development as well; the app and its development window use the same modules.

## Package contract

Packages live in a flat directory. The manifest's `name`, its folder name,
dependency keys, and import names are identical. For example,
`packages/my-patch/package.toml` contains:

```toml
api = 1
name = "my-patch"
version = "0.1.0"
default_gamemode = "custom"

[[editor]]
name = "custom"
path = "./editor"

[[gamemode]]
name = "custom"
path = "./gamemode"

[dependencies]
"bn6" = "0.1.0"
"editor-common" = "0.1.0"
```

Names are single folder names using letters, digits, dots, underscores, and
hyphens. The directory also contains its scripts and any files they need.
The UI name comes from `package-name` in the package's locales, falling back
to `name`. There are no separate IDs, display-name fields, package kinds,
dependency aliases, asset/module lists, or `extends` field. Every ordinary
`require("@package")` loads that package's `init.luau`.

A package can declare independent `[[editor]]`, `[[gamemode]]`, and
`[[telemetry]]` exports alongside reusable modules and data. Names are unique
within each category; an editor and a gamemode may both be named `custom`.
Each `path` resolves from the package root with the same `.luau` and
`init.luau` rules as `require`. It cannot point into another package; a local
module can import and re-export dependency modules. All declarations are
checked, including unselected exports in dependencies. There is a combined
limit of 64 exports per package. Training scripts are not part of this API.

Each category with exactly one export selects it automatically. Otherwise,
choose an editor with `--editor NAME` and ROM preparation with
`--gamemode NAME`. `check` lists every export without selecting one. The portable API uses
`Profile::with_editor`, `with_gamemode`, and `with_telemetry`, or
`with_export(ExportKind, name)`. Selections are independent and included in the
environment digest. Locale and input changes preserve them. Dependencies do
not implicitly supply exports to a consumer.

Each module returns its capability directly: an `Editor`, `GameMode`, or
`Telemetry` table. Loading an editor does not execute the gamemode or
telemetry module. Each may provide its own `detect_rom` for automatic
recognition. Package tests remain in `init.luau`'s `test` function and run
without selecting any capability.

For example, `my-patch/editor.luau` can reuse the editor:

```lua
--!strict
return require("@bn6/editor")
```

A dependent package can also wrap the existing methods directly. For example,
its editor module can replace one chip name while using BN6's existing reader,
editor controls, validation, and graphics:

```lua
--!strict
local rom = require("@bn6/editor/rom")
local open = rom.open
rom.open = function(bytes, wram, definition, characters): rom.Assets
    local assets = open(bytes, wram, definition, characters)
    local chip = assets.chip
    assets.chip = function(id: number): rom.Chip?
        local original = chip(id)
        if not original or id ~= 1 then return original end
        local result = table.clone(original)
        result.name = "Custom Cannon"
        return result
    end
    return assets
end
return require("@bn6/editor")
```

The reader's `from_inputs` method dispatches through its public `open` method,
so the same replacement reaches the existing consumers. Text-encoding changes
can pass a charset as `open`'s fourth argument. Other changes replace methods
such as `chip_is_legal`, `navicust_part`, or `patch_card` in the same way. There
is no separate override data schema. Use the dependent package's translator
for localized replacement text. Module instances are local to a callback's
package graph; the wrapper is reapplied once for each fresh callback and does
not alter other documents or the installed dependency. Chained extensions
control their order through ordinary imports.

Its independent `my-patch/gamemode.luau` supplies ROM preparation:

```lua
--!strict
local mode: GameMode = {
    patch_rom = function(bytes: buffer): buffer
        return tango.bps(bytes, tango.read_file("changes.bps"))
    end,
}
return mode
```

An extension can import another gamemode and call its `patch_rom` before
applying its own transformation. Code determines the order; the host does not
choose a parent or apply dependency patches implicitly. A missing `patch_rom`,
or a package with no gamemodes, leaves the ROM unchanged. Opening a document
requires an editor and does not require a gamemode. Preparing a ROM or opening
a gamemode program uses the declared default unless one is explicitly selected.

A playable gamemode provides `setup(context, game)`. Register callbacks at
instruction addresses and use the game handle directly inside them:

```lua
--!strict
return {
    platform = "gba",
    setup = function(context: GameModeContext, game: Game)
        local target = 0x08000100
        game.hook(0x08000200, function()
            game.write_register("thumb_pc", target)
            local bytes = game.read("main", 0x02000000, 4)
            buffer.writeu32(bytes, 0, context.player)
            game.write("main", 0x02000000, bytes)
            game.ready()
        end)
    end,
} :: GameMode
```

The context contains a 1-based player position, a 16-byte seed as a 1-based
array, the host's boolean `disable_bgm` setting, and named scalar package options.
`disable_bgm` is frozen per player and recorded with the match, separately from
package-defined options. The package owns the meaning of those options.
Each selectable match type is its own `[[gamemode]]` export, so identity and
selection do not depend on a nested mode option. Related gamemodes can import a
shared implementation. Omit `setup` for an export that only prepares ROMs.
The desktop discovers playable exports with `platform = "gba"` and a matching
`detect_rom`. Labels come from `gamemode-<export name>` in the package's
`locales/<locale>/package.ftl`, falling back to the export name. The package's
`default_gamemode` determines the initial selection. Every
package with gamemodes must declare that default in `package.toml`. Removing a
selected package invalidates readiness instead of switching to native behavior.

`game.hook(address, callback)` is available during setup. Memory and register
operations, plus `ready()`, `round_started()`, `round_outcome(winner)`,
`match_ended()` and `match_aborted()`, are available inside callbacks. A winner
is a 1-based player position, or nil for a draw. Callbacks return nothing.
The backend defines the memory spaces and register names.

Reads observe earlier writes to the same memory space or register in that
callback. Write buffers are copied when passed to `game.write`. The host
stages operations internally and validates the complete callback before
committing any writes or lifecycle events. A script or validation failure
commits nothing.

There is no separate gamemode state buffer. The emulator's memory and registers
hold changing game state and are already captured by rollback. Callbacks may
capture setup constants derived from the ROM, seed and settings. Setup and its
closures are recreated for every callback using cached compiled code; mutable
Lua locals do not persist across invocations. Checkpoints verify the immutable
package/input identity, player, seed and options before restoring a simulation.

A gamemode can register at most 256 unique instruction addresses. Each callback
can issue at most 4,096 writes, 4 MiB of write buffers and 128 lifecycle calls.
Memory reads use the same bounds as telemetry, and register reads allow at most
4,096 calls per callback.

`tango_backend_mgba::gamemode::boot` boots a linked pair directly from two
gamemode programs, ROMs and saves, without native game registration. It runs
until both programs report `ready`, with a 3,600-frame limit and cancellation.
The GBA adapter exposes raw `main` memory reads, work-RAM writes, `r0` through
`r14`, read-only `cpsr`, and `thumb_pc`. Instruction hooks run after the original
Thumb instruction at aligned addresses in the loaded ROM. PC redirects must
also remain inside that ROM. Other instruction sets and IO writes need backend
capabilities before scripts can use them.

The adapter bounds each player to 4,096 hook calls and 4,096 queued lifecycle
events per frame. Startup retains at most 4,096 pending events per player
across the walk, consuming readiness signals immediately. It
validates events and writes together, stops on a failed hook, and publishes no
lifecycle events from a failed frame. Rollback captures include emulator state,
the gamemode identity, readiness, lifecycle state and telemetry pollers. Lifecycle history is shared
between captures until it changes. Playback, seeking, export and live rollback
propagate frame failures without publishing that frame or advancing the cursor.

`bn6` exports three independently selectable gamemodes: `single`, `triple` and
`random`. Each supports the four revision-zero US/JP cartridges; select one with
`--gamemode single` (or `Profile::with_gamemode("single")`). The package's default
is `triple`. Hooks read `context.disable_bgm`; there is no `battle_type` option. Startup preserves
the game's link handshake, and random battles hand control to the players at rank selection.
The gamemode's addresses, RNG initialization, menu behavior and lifecycle hooks live
under `bn6/gamemode`, with shared RNG derivation in `bn-common/gamemode`.

`PreparedGameMode::prepare(profile, engine, rom, disable_bgm, options)` applies ROM patches
once and freezes the effective ROM, options, selected gamemode and entire package
graph. Its portable identity includes every package's exact version and content
digest, the transformed ROM's SHA3-256, the backend ABI and script ABI. The GBA
backend supplies its ABI through `tango_backend_mgba::gamemode::runtime()`.
`program(player, seed)` recreates callbacks against those frozen inputs, without
reapplying patches or consulting a changing package catalog.

A prepared gamemode implements the game-neutral `tango_match::gamemode::Factory`.
`tango_backend_mgba::gamemode::Backend::new` accepts two factories in absolute
player order and implements the ordinary `Backend::start` and `open_replay`
interfaces, without a native game registration. Replay workers can install the
programs on fresh cores and restore a primed capture, including script and
lifecycle state. Pass the prepared ROMs to these session interfaces; legacy
`match_type` and `disable_bgm` fields must remain at their defaults. Package
behavior comes from the options frozen into the factories.

`tango-session` accepts `SessionBackend::Owned(backend)` directly for a package
match or replay; it has no dependency on native game registrations. The host
supplies prepared cartridge images and keeps optional artwork and game labels
beside the session. Replay playback uses `ReplaySessionArgs`, and validates the
backend against both recorded configurations before opening it. Native save
conversion for unported games remains in the host's legacy
support path. `Session::backend()` exposes the actual engine's timing and inputs.
ROM discovery indexes exact bytes by content hash, including files the native
registry does not recognize. Package matching and peer/replay resolution inspect
that catalog directly. Native metadata is optional in both sessions and embedded
editors. Playback, analysis and export share the same ROM/package resolution.
The main picker resolves package games and saves through this catalog too; it
requires no native family or save model. Removed packages, ROMs, or selected saves
leave an error and cannot become a different ready loadout implicitly. Singleplayer
and native training still require a native registration; package entries currently
expose the scripted editor and PvP flow.

Simulation callbacks use `en-US`, independently of the UI locale. Preparation
clears other capability selections and replaces editor inputs with only the ROM;
changing the selected editor or its locale therefore does not change the
simulation identity. All files in the gamemode's dependency graph are still
pinned, including files not read on a particular execution. Packages can load
files dynamically, so it would be incorrect to hash only observed imports.
An independently selected editor extension is outside that graph.

`configuration()` returns the identity together with its option values.
`Catalog::resolve_gamemode(configuration, engine, rom)` reproduces that
configuration using the exact installed package version, this host's backend ABI
and a base ROM. Changed content, options or ROM bytes fail verification; missing versions
do not fall back to newer releases. Verify each seat against the same seat's
identity: two different cartridge variants can legitimately have different ROM
digests. Identity verification alone does not establish compatibility between
two different gamemodes. Seed and player position are bound later, when opening
the program.

Lobby protocol `0x58` carries the configuration, and each save commitment binds
the sender's exact simulation settings. A material settings change invalidates
readiness; a reveal for different settings is rejected. The configuration codec
preserves numeric option bits, including signed zero. Options have a combined
16 KiB name/value budget; the encoded configuration is bounded to 64 KiB.

A gamemode may implement `import_replay(legacy: LegacyReplay, rom: buffer):
ReplayImport?` for recordings from before packages. It returns `nil` unless it
can reproduce that exact legacy game/version, mode codes, and cartridge. A
successful result contains optional `disable_bgm` and `options`; the host
prepares the actual installed export and pins its code, dependencies, engine,
and effective ROM. Imports run in English with only the ROM as a host input.
The old `rom_variant`, `match_type`, and `match_subtype` values retain their
encoded values at this compatibility boundary.

The desktop imports only when both seats have an unambiguous match. It changes
a copy of the metadata in memory, preserving nicknames, timestamp, seed, SRAM,
inputs, and perspective; the source file is unchanged. Playback, analysis,
export, and setup previews share this resolution. Package recordings always
require their recorded identities and never fall back to a legacy importer.
The replay index keeps readable recordings even without native registrations.

BN6 accepts unpatched simulation-version-0 recordings for its three gamemodes,
with the original US/JP Gregar/Falzar cartridge checksums and mGBA adapter revision
2. Unsupported versions, old subtype codes, patches, or altered cartridges are
not imported. `tango.crc32(buffer)` supplies bounded native CRC32 for recognition;
checksums and compatibility decisions stay in the package.

Package replays use container `0x1F`, with both configurations in absolute player
order. Native replays retain `0x1E`. Partial gamemode declarations, native/package
identity mixtures and header downgrades are rejected. The package version also
keeps older readers from ignoring the gamemode metadata and simulating it with
native support. The SRAM and input stream layouts are unchanged.

Desktop matchmaking, replay playback, analysis, and export resolve both frozen
configurations against the installed package catalog and scanned ROMs. Missing
or changed content fails resolution; it never falls back to native support.
The browser currently rejects package matches and recordings because it has no
script runtime. The term **ruleset** remains reserved for folder limits.

Telemetry modules export `poll(state, context, memory): TelemetryFrame`.
An optional `initial_state(): buffer` callback defines the private state
layout; `Profile::initial_telemetry_state` calls it, or returns an empty buffer
for a stateless poller. The context contains the simulation tick, the round
number from the match's rollback-consistent lifecycle, and a 1-based player
position. Output contains named scalar values and named events with scalar
fields; the host does not define game-specific metrics.

`Profile::poll_telemetry_with_memory` supplies a borrowed reader for the current
emulator state. `memory.read(space, address, length)` returns an independent
buffer from a host-defined address space. Addresses are unsigned 32-bit byte
addresses, not collection positions. Reads cannot write emulator memory. Each
read is limited to 1 MiB and the entire poll to 4 MiB and 4,096 calls, including
zero-length reads. Invalid ranges fail before calling the host. The host must
use side-effect-free memory reads; the borrowed callback expires before the
poll returns and cannot survive into another tick. This avoids copying entire
memory banks for a few telemetry fields.

ROMs and other immutable inputs remain available through `tango.read_input`.
`Profile::poll_telemetry` also works for input-only pollers; trying to read live
memory without a supplied reader fails. Both entry points return a frame and
updated private state. Hosts restore that state with the emulator on rollback.
Replaying the same memory and state reproduces the same sample and events;
module globals reset between calls. Polling never loads the editor or gamemode.

Telemetry output is decoded with Serde and bounded to 4,096 values, 64 KiB,
five levels of nesting and 4 KiB per string; state is limited to 64 KiB.
Malformed output and memory-read failures leave the caller's state unchanged.
The desktop selects a telemetry export independently of the editor when exactly
one current export recognizes an unpatched ROM. The GBA backend exposes `"main"`
as its address space. Each sampler's private state and source identity travel
with emulator checkpoints, including captures shared between replay workers.
Sampling failures stop that sampler and become error records; they do not end
the match. Restoring an earlier checkpoint permits the sampler to run again.

`tango-match::telemetry::stream` carries the game-neutral frames. Its pending
queue is bounded to 16 MiB. Live sessions consume only confirmed ticks; replay
playback drains observations as it advances. The separate replay analysis pass
builds a timeline that is unaffected by playback seeks. Both session types
expose their timeline through `records()`. The timeline keeps exact changes,
missing-value gaps, events and errors up to 64 MiB, then reports `limited_at`
instead of silently dropping earlier history. Its work per tick depends on
currently reported fields, not the number of historical field names.

Telemetry exports may provide `view(history, context, state) -> Node` and
`update_view(state, action) -> ViewState`. These use the same generic UI tree,
controls, canvas drawing, and i18n as editors. View state is separate from the
sampler's rollback state; a failed action leaves both the old view and its state
intact. The host retains the exact ROM-bound profile that produced the records,
so a package rescan cannot change an existing match's results.

`history.value({player, name, tick})` returns a scalar or `nil` at a missing
observation. `history.series({player, name, from_tick, to_tick}, resolution)`
returns up to 2,048 numeric buckets. Each has inclusive tick bounds, optional
`first`, `last`, `min`, `max`, and a `gap` flag; extrema retain brief spikes,
and gaps prevent connecting discontinuous readings. Queries return gaps outside
the collected extent. `history.count_events` takes the same range and counts
matching event names. Each query kind permits 64 calls per view; series queries
share an 8,192-bucket budget. History is read-only and available only during the
callback. Arguments and results use Serde, including `nil` for missing values.

The context carries a 1-based local `player`, inclusive tick range,
`ticks_per_second`, and a host-derived `incomplete` flag for failed or limited
collection. The desktop embeds the optional view on its post-match card. BN6's
view provides HP/position selection, exact hover values, chip-use counts, and
English/Japanese translations. It imports presentation lazily, so per-tick
polling does not load UI modules. Its static graph uses cached rasterization.

Replay-library charts, outcome summaries, and round lifecycle still use native
telemetry; their remaining presentation and lifecycle paths are being migrated.
Legacy patches retain native telemetry until their package metadata is available. DS and browser sessions do not run scripted
telemetry yet.

`bn6/telemetry/` implements unit HP and position, per-player custom-screen
state, and chip-use events. `bn-common/telemetry/hand.luau` owns the shared
hand-cursor tracker, including duplicate uses, new hands, KO cleanup and round
resets. The BN6 reader makes three reads totaling 447 bytes per valid sample;
invalid/uninitialized units produce no values or events. `bn6/rom/detect.luau`
is shared by its independent editor and telemetry exports.

The bundled `editor-common` library supplies generic buffer, field, and
presentation helpers. `gba` supplies script-owned memory mapping and
4bpp tiled graphics with BGR555 palettes. It calls the native `tango.unlz77`
capability for BIOS LZ77 decompression; game-specific pointer conventions stay
in the package.

Editors may optionally expose `templates(): {SaveTemplate}` and
`create_save(name: string): buffer` together. A template has a stable `name` and
localized `label`; the host binds the selected ROM and locale before listing it.
The returned buffer is the complete encoded save file, checked by the editor's
decoder before the host creates it. This powers New Save in the desktop picker.
Editor selection is independent of gamemode selection, and an explicitly chosen
editor version stays pinned until the user changes it.

`bn6` declares `./editor` directly. `bn6/init.luau` exports reusable editor
code, translations and package tests. Its `editor/` directory contains the
editor entry and component directories: `save/` owns format/model code and save
fixtures, `rom/` owns assets and message decoding, `folder/` owns controls and
editing operations, `build/` owns validation, and `navi/` owns effects and stats.
Tests live beside their components. Editor translations live in `editor/locales`;
the root `locales` directory contains only package metadata.

`bn-common/folder/` contains the reusable folder catalog, rules, edit
operations, interaction state and their tests. Its `ui/` subdirectory composes
the shared folder panes. The library's root entry exports its translator and
test entry point; shared translations stay in the root `locales/` directory.

Imports resolve relative to the importing module or through `@` followed by a
declared dependency's package name:

```lua
local folder = require("./folder")         -- folder.luau or folder/init.luau
local model = require("../model.luau")     -- parent within this package
local common = require("@editor-common")   -- dependency's init.luau
local fields = require("@editor-common/fields") -- dependency submodule
```

Use the standard global `require`, with the same resolution in the type checker
and runtime. There is no `tango.require` alias. The `tango` namespace supplies
host capabilities such as file access and translation catalogs.

Package code and host APIs use `snake_case` for functions, variables, and table
fields, `PascalCase` for types, and `SCREAMING_SNAKE_CASE` for constants. Luau
built-ins and external format names retain their original spelling.

Manifest package names and module paths are case-sensitive. The `@package`
alias is ASCII case-insensitive, following Luau; dependency names cannot differ
only by case within a manifest. Use the manifest's spelling in source.
Module paths use `/`, and may omit `.luau`. Directory modules resolve to `init.luau`; if both
a file and directory module match, use an explicit path to disambiguate.
Relative paths are based on the importing file, including inside `init.luau`.
There is no ambient search path, global module
registry, or implicit access to transitive dependencies. Each imported module
keeps its own package's `tango.read_file` capability, even when its functions are
called by another package. Imports share a cache within a VM operation.

The checker and VM use the same resolved graph and immutable source bytes.
Direct static imports carry actual exported values and type aliases: for
example, `local common = require("@editor-common")` supports `common.Options` when that
module exports `type Options`. Missing or escaping static imports are errors.
Capability imports retain extra values such as `t` and exported type aliases
while the checker verifies each declared capability interface.
Indirect/computed imports retain the same runtime sandbox, but cannot provide
statically known exports. Every `.luau` file is strictly checked, including
unused modules. Packages with dependencies are syntax-checked on ingestion and
fully type-checked during profile resolution, before any code executes.
The host also validates every returned value and translation argument at runtime.
Like stock luau-lsp, the upstream new solver can miss invalid values in inferred
record literals passed to dictionary parameters; an explicit `TranslationArgs`
annotation catches those mistakes while authoring shared translation arguments.

For editor support, open the repository root with stock
[Luau LSP](https://github.com/JohnnyMorganz/luau-lsp). The workspace settings load
the SDK definitions and enable file-relative imports inside `init.luau` through
`luau-lsp.require.useOriginalRequireByStringSemantics`. The server resolves
extensionless imports to `.luau` files; `.lua` in an unknown-import error is its
fallback path when the `.luau` candidate does not exist at the resolved location.
Zed uses [.zed/settings.json](../.zed/settings.json) with its
[Luau extension](https://github.com/4teapo/zed-luau), while VS Code uses
[.vscode/settings.json](../.vscode/settings.json). Both load `sdk/tango.d.luau`,
enable the new solver, and use the same `packages/.luaurc` aliases. Zed's extension
expects its own options under `settings` and server options under
`settings["luau-lsp"]`; its definition-file list is converted into startup
arguments. Restart the Luau language server after changing these settings.
The VS Code extension reads `luau-lsp.types.definitionFiles` when starting the
server; use **Luau: Reload Language Server** after changing these definitions.
Other editor clients must pass the SDK as a startup argument too. For example,
from the repository root:

```sh
luau-lsp lsp --settings=.vscode/settings.json --definitions:tango=tango-script/sdk/tango.d.luau
```

`analyze` reads definition paths from its settings file directly, whereas `lsp`
requires this startup argument. For clients that start in a different working
directory, pass absolute paths to the settings and SDK files.

[packages/.luaurc](../packages/.luaurc) supplies ordinary
[Luau aliases](https://rfcs.luau.org/require-by-string-aliases.html):

```json
{
  "aliases": {
    "bn6": "./bn6",
    "bn-common": "./bn-common"
  }
}
```

Keep this file above the package directories: stock Luau LSP looks in the parent
of an `init.luau` directory for configuration. Add new package names to this map
alongside their folders. The bundled-package suite checks that the map stays in
sync. No custom language server, source transformation, or LSP build helper is
needed. To check all authoring sources with an installed server:

```sh
luau-lsp analyze --settings=.vscode/settings.json packages
```

Aliases tell the editor where source files live; stock Luau LSP does not read
`package.toml` dependency permissions or versions. Tango's `package check`
uses the manifest-selected dependency graph for those checks and for execution.
Runtime imports never read `.luaurc` or live filesystem paths. Relative imports
cannot leave their package, and `@package` imports cannot traverse above the
selected dependency's root or access undeclared packages.

The complete current types are in [sdk/tango.d.luau](sdk/tango.d.luau). Checking
also verifies the returned editor's contract. `decode` converts the file to a
mutable document buffer; `encode` reverses that operation; `validate` returns
user-facing diagnostics. `view` returns a declarative tree and `update` mutates
the document and string-keyed view state in response to an offered action.
Their signatures are `view(bytes, state, context)` and
`update(bytes, state, action, context)`. The host-supplied `EditorContext` contains
`read_only`, an optional `session`, and host-supplied `embedding` preferences and
action capabilities; callbacks that do not need them can omit the final parameter.
Embedders use `Document::open_with_context` to set this capability. Mutating the
callback's context table or storing a different value in view state cannot
change it: a read-only update that changes document bytes fails atomically,
preserving the old bytes, view state and rendered tree.
The package entry's `test` is optional, but the `test` command reports an error
if none is supplied. An editor can export its own test and `init.luau` can
delegate to it.

### Edit sessions

Embedders use `EditorSession::open(profile, input, editable)` for the whole-save
lifecycle. It starts in view mode even when editing is permitted. Its document
is exposed read-only through `document()`; actions go through the session.
Scripts receive `context.session` with host-owned `editable`, `editing`,
`saving`, and `can_save` flags. `read_only` also becomes true outside edit mode.
A package cannot grant itself edit access by changing this context table.

`EditorSession::configure(locale, editable, embedding)` refreshes all panel
options atomically. Identical options do not run scripts, so the embedder can
call it while constructing each view, including during a pending write.
Revoking edit permission preserves staged bytes and history but disables
mutations, Save, Undo and Redo. Cancel remains available, and granting permission
again resumes the draft. A failed reconfiguration preserves the old view and
permissions; an embedder must withhold interaction when it cannot apply newly
requested restrictions.

An update requests a transition with `{kind = "edit", command = "begin"}`,
`"cancel"`, or `"save"`. At most one edit transition is allowed per update.
The `editor-common/session` module supplies composable controls and action
handling; labels come from the consuming package's translator. Embedders with
external controls can call `EditorSession::edit` through the same capability
checks. Bare `Document` instances do not handle edit-transition effects.

Cancel restores the last successfully saved model across every section. The
optional `editor.reset_view(state)` callback supplies state for Begin, Cancel, and
completed Save, retaining package-owned view preferences and discarding scratch state. Without
it the host clears view state. BN6 retains the selected section, grouping and sort
orders and the streamer cover's revealed state, while clearing filters, picker
state, pending tags and held parts.
These transitions clear undo/redo history.

`dispatch`/`edit` returns a `SessionUpdate` containing clipboard effects and an
optional `SaveRequest`. The embedder writes `request.bytes()` to its own storage,
then calls `finish_save(&request, written)`. The receipt belongs to that exact
attempt and session; stale, duplicate and foreign receipts are rejected. While
the write is pending, further actions and locale changes are rejected. A failed
write restores the staged view and history for retry. Success adopts the exact
model decoded from the written bytes, leaves edit mode, and establishes the new
Cancel baseline. Both outcomes are prepared before returning the write request,
so a post-write package callback cannot leave a successful save unacknowledged.

`Document::snapshot()` serializes a private copy for session launch without
marking it saved. The package's encoder repairs checksums and the decoder checks
the resulting format. Build diagnostics remain advisory for this operation;
`encode()` and the Save transition still enforce legality. Rust has no knowledge
of the save's checksum or layout.

### Host capabilities

`EditorSession::open_embedded` accepts an `Embedding` with `inline_actions`,
`streamer_mode`, and a map of host actions. `set_embedding` updates this context
without resetting edits or view preferences. Streamer mode defaults to false;
the package decides how to present its cover and handles Review as ordinary
view state. The development editor's `--streamer-mode` flag exercises this flow.
BN6 retains Review across edit/locale/preference changes and covers each newly
opened document. Its regional logos live in `bn6/editor/cover/logos/`, and the
shared `editor-common/cover` module composes the artwork and reveal button.

`tango.unlz77(source, offset?)` reads a type-0x10 BIOS LZ77 stream at a zero-based
offset (default zero), returning a new buffer or `nil` for malformed data. It
supports overlapping copies and truncates the last run to the advertised output
length. Inputs are limited to 32 MiB and the format caps output below 16 MiB.
The host charges its work budget before allocating; invalid offsets and exhausted
budgets raise errors. Reading a stream does not copy the whole source ROM.
`tango.lowercase(text)` supplies Unicode lowercase for filtering, independent
of locale, with a 16 KiB input/output limit.
`tango.decode_png(buffer)` returns a `Raster` with RGBA8 pixels, preserving alpha.
It decodes the first frame, converts supported PNG color formats, and returns a
fresh pixel buffer. Compressed input, decoded data, and RGBA output are each
limited to 32 MiB, with dimensions of 1..=4096. Limits apply before decompression
and allocation; malformed inputs also spend the shared host-work budget.
`tango.encode_png(raster)` returns a PNG buffer from RGBA pixels, preserving alpha;
`tango.base64_encode(buffer)` returns standard padded Base64. These encoders charge
the same host-work budget as other capabilities. PNG input pixels and output,
and Base64 output, are each limited to 32 MiB; raster dimensions are 1..=4096.

`update` may return `nil` or a sequence of `Effect` requests:

```lua
return {{kind = "copy_html", text = "Plain fallback", html = "<b>Rich text</b>"} :: Effect}
```

Clipboard kinds are `copy_text`, `copy_html` (with required plain-text fallback),
and `copy_image`. An image is either a `Raster` or a `Drawing` with
integer `width`, `height`, drawing `commands` and an optional `corner_radius`.
This lets a package compose exported artwork using the same generic display list
as its UI. The Iced host rasterizes drawings and shares the image cache.

`Document::dispatch` returns the effects only after the update, effects and new
view validate and the transaction commits. A failed update returns no effects
and preserves the document. Read-only views can request copies and scrolling. Undo, redo,
validation and locale/view refresh never replay effects. The embedder executes
the returned requests once and reports transport failures separately from the
already committed transaction. The development host falls back to plain text if
its clipboard backend cannot publish HTML.

`{kind = "scroll_to", id = "body", x = 0, y = 0}` scrolls to relative offsets
in `0..1`. The renderer maps IDs into its own document namespace; a script cannot
address the shell's scrollables or another editor. The host executes these with
`Renderer::scroll_to` after committing the update. Missing targets are harmless,
such as when a tab switches from a shared viewer scroll to an editor with its
own scrolling panes.

One update can return at most 16 effects, with a 4 MiB limit per string and shared
64 MiB limits for decoded output and image pixels. Drawings share the UI's command,
point and rasterization-work limits. Scripts have no direct clipboard access.
Buttons can supply `feedback = {child = ..., tooltip = ...}`. After successful
host work, `Renderer::acknowledge(action.id)` displays this passive content for
1.5 seconds; the host uses Tango's active-animation frames subscription to refresh
it. Feedback is scoped to the renderer, cannot dispatch its own actions, and
shares the ordinary view's resource limits. The `editor-common/clipboard` helper
composes Tango's copy icon, checkmark, tooltip and spacing from these primitives.

UI tables, drawing commands, styles, and events use Serde-derived structs and
tagged enums with mlua's Serde conversion. Resource preflight checks cycles,
metatables, collection shape, nesting, strings, and total allocation bounds;
typed validation then checks IDs, image dimensions, selections, and ranges.
Rust does not hand-decode each widget's fields or dispatch on string tags.

Every node can carry a `motion` descriptor:

```lua
node.motion = {key = "detail", revision = state["detail_revision"] or "0",
    from = {0, 20}, duration_ms = 160, easing = "ease_out_cubic"} :: Motion
```

This slides its subtree into place. Layout keeps its resting dimensions;
drawing and pointer handling move together, using Tango's native floating widget.
Each key owns a renderer-local timeline. A new key or changed descriptor starts
an entrance; unchanged descriptors keep their elapsed time across document
updates, hover, scrolling and redraws. Removing a descriptor releases its clock.
Use a fresh `Renderer` for a newly opened document. Animation frames emit `None`
presentation messages and request redraws automatically; the embedder refreshes
the view without calling Luau. They never change document state or run ROM decoding.
`Renderer::view_at` samples a fixed monotonic time without scheduling redraws,
for captures and tests; call it with the complete view to retain all its clocks.
`restart_motion()` restarts presentation for a retained document without changing
its data or view state. Rendered elements own their text, callbacks, canvas data
and image handles, allowing an embedder to release a session lock before Iced
uses the view. Image caches and timeline maps remain scoped to the renderer.

The default duration is 160 ms, delay is zero, and easing is `ease_out_cubic`.
Other easings are `linear`, `ease_in_cubic`, and `ease_in_out_cubic`.
Duration and delay are each at most 60,000 ms; a zero duration settles immediately.
`from` contains two pixel offsets in ±16,384. Keys must be nonempty and unique
within the view, including passive content; keys and revisions are each limited
to 1024 bytes, with at most 1024 clocks. `editor-common/motion.enter` supplies
the usual entrance descriptor without changing its input node.

`tango_script::ui` is the renderer-independent schema and validation layer. The
separate `tango-script-iced` crate implements its Iced backend using `tango-ui`;
the package runtime and type checker do not depend on a GUI toolkit.

Nodes can use the standard `tone` names, `{row = index, selected = boolean}` for
1-based alternating/selected rows, `{tint = color}` for colored controls, or
`{tab = active}` for tab buttons. The `tooltip` tone supplies native popup chrome.
Buttons use Tango's existing interaction styles, including gradients, borders,
shadows and disabled states. A row tone on a non-button supplies its static
appearance. `appearance` overrides individual background, text color, border
color/width, and corner radius properties without replacing the rest of the
preset. Buttons additionally accept `hovered`, `pressed`, and `disabled`
appearance overrides, applied after their ordinary appearance.
Backgrounds accept a color or `{angle = radians, stops = {{offset = 0, color = ...}, ...}}`.
Linear gradients have 1–8 stops with strictly increasing offsets in `0..1`;
each stop supports the same theme, tint and alpha colors as other UI nodes.
Button children may contain controls; an inner control handles its own click.
`action_enabled = false` disables only the enclosing button's action, while
`enabled = false` disables its entire subtree.

Scroll nodes accept an optional `id`, `on_scroll = true` to publish typed
`scroll` actions, and `scrollbar = "auto" | "hover" | "hidden"`. Events carry
normalized `x`/`y` offsets; an axis whose contents fit emits zero. The shared
`editor-common/tabs` module composes compact tab buttons, optional error badges
and tooltips, a horizontal strip with passive overflow fades, and trailing
controls from these ordinary nodes.

Colors are theme names, four-channel RGBA arrays, or `{theme = name, alpha = n}`.
Text inherits its surrounding foreground unless its own `color` is supplied;
the normal font inherits the embedder's default font. `padding` surrounds a
node, while `content_padding` controls the internal padding of buttons, inputs
and choices. Those controls accept the usual `size` text metric.
Text nodes also support `strikethrough` and `underline` decorations.
Images accept `opacity` from zero to one. Canvas paint supports `line_cap`
(`butt`, `square`, or `round`); plain rectangle fills use Iced's crisp rectangle
rasterization, while rounded rectangles and paths have antialiased edges.
`rasterize = {corner_radius = 0, nearest = false}` bakes a canvas at its declared
integer pixel dimensions before displaying it with image-style aspect fitting.
It supports every drawing command, including text and images. The optional
corner radius is a hard alpha mask in source pixels. Rust uses the same generic
drawing commands for live vectors and cached software rendering; scripts supply
the artwork. Cached drawings include their contents, font and theme in their
identity and share the 64 MiB image cache. Source dimensions are at most 4096²;
generated images count toward the view's 64 MiB image budget, with a separate
work budget of 67 million estimated pixel operations per view for rasterization.
Canvas `scale_to_fit` defaults to `true`. Set it to `false` to keep drawing and
pointer units in display pixels when layout constrains the widget. Rasterized
canvases then draw at native size from the widget's top-left corner.

Nodes can set a `cursor` hint, such as `grab`, `pointer`, `crosshair`, or `text`,
without capturing input. Disabled nodes ignore that hint. Reorderable lists use
Tango's shared drag styling. The host gives the root node a bounded viewport;
scripts use `scroll` nodes to choose which regions scroll independently.
Interactive canvases receive pointer observations even when they allow the
event to propagate. `pointer_capture` defaults to `true`; use `false` to pass
all events through, or filters such as `{{phase = "down", button = "left"},
{phase = "wheel"}}` to capture selected gestures. This lets an editor consume
wheel rotation while holding an item and leave scrolling available otherwise.

`tooltip` accepts a string or a rich `{content = node, position = "follow_cursor",
gap = 8}` description. Other positions are `top` (the default), `bottom`, `left`
and `right`. Scripts compose artwork, text and tooltip chrome from ordinary
nodes. Tooltip content is presentation-only: actions inside it cannot dispatch.
It shares the enclosing tree's ID/key namespace, depth and resource budgets.

Actions have an `id` and a `kind` discriminator: `change`/`activate` carry a
string `value`, `toggle` carries `checked`, `slide` carries `number`, and
`pointer`, `key`, and `reorder` have typed payloads. Narrow on `action.kind`
before accessing the payload. Reorder indices are 1-based positions in the
list's `children` array. Canvas coordinates are in the declared logical dimensions
by default; rasterized canvases also
account for aspect fitting and letterboxing. With `scale_to_fit = false`, they
remain in display pixels. Wheel deltas use the same units, with a line interpreted
as 16 display pixels. Keyboard input goes to a canvas after a click.

Each resolved package graph retains one VM and its compiled code. Operations
take exclusive access and recreate module environments, captured variables,
import results, host bindings and quotas. Only explicit document bytes and view
state survive as mutable script state; the profile's immutable inputs remain available.
`validate` and `view` run separately on copies, so rendering cannot mutate
the stored document. A failed update or view rebuild leaves the previous revision
intact. Encoded files must decode and validate again before the host writes them.

Dependency resolution permits up to 128 packages, including multiple exact
versions of a library through different transitive dependency paths. Each
package declares one exact version per dependency name. Missing, duplicate, and cyclic
package references fail. Automatic version solving is not implemented yet.

`tango.read_file("path/to/file")` reads any file beneath the executing module's
package root, including `package.toml` and source files. Paths are resolved in
an immutable file snapshot: traversal above its root, absolute paths, platform
prefixes, and backslashes are rejected. Unicode names and embedded spaces work.
The API returns a fresh buffer, so edits cannot modify the package snapshot.

`tango.input_size("rom")` returns the length of an explicitly supplied input,
or `nil` when absent. `tango.read_input("rom", offset, length)` returns a fresh
buffer containing that range. Byte offsets are zero-based; lengths are counts.
Both must be nonnegative integers; missing inputs, fractional/non-finite numbers,
out-of-bounds ranges, and reads larger than 32 MiB fail. Slot names are not filesystem paths. Every
module in the selected profile receives the same input snapshot.

Hosts bind inputs with `Profile::with_inputs(Inputs::new(...))`.
The development `edit` and `test` commands accept repeatable `--input NAME=PATH`.
`--rom PATH` runs the selected gamemode's composed `patch_rom` transformation once
and exposes the effective bytes as `rom`. Subsequent callbacks, undo, and save
operations read that snapshot. `--input rom=PATH` supplies already prepared bytes.
Reload starts a new document environment and rereads the chosen inputs.
Inputs are never included in package archives or written back by scripts.

`.tangopkg` is a deterministic ZIP of that entire file tree. The loader rejects
traversal, symlinks, special files, duplicate entries, file/directory conflicts,
and expansion beyond its limits. Native directory collection uses open directory
capabilities with symlink following disabled; concurrent symlink replacement
cannot redirect reads outside that root. Browser adapters snapshot their virtual
store. All captured files are bundled, regardless of whether scripts use them.

Package identity hashes every file path and its exact bytes, including the raw
TOML manifest. Profile identity also hashes the resolved graph, SDK, input names
and bytes, the active locale, and pinned interpreter version. **This is an editor
identity, not a netplay compatibility claim.** Compression does not change
identity; changing any captured file does.

## Localization

Packages own their translations in catalog directories containing
`<language>/*.ftl`, using the same Fluent format as Tango. `locales/` is the
conventional directory; packages may keep several catalogs at other paths.
FTL files are parsed, validated, hashed and bundled alongside other package
files; there are no manifest declarations. Locale folder
names use canonical language identifiers such as `en-US`, `ja-JP` and `zh-TW`.
Multiple files in one locale share a catalog; loading a catalog validates its
language directories and rejects duplicate messages, terms and attributes.
The conventional `locales/` catalog is loaded eagerly for metadata: its optional
`package-name` message localizes the package's UI name, falling back to
the manifest's `name`.

```ftl
# locales/en-US/main.ftl
items = { $count ->
    [one] One item
   *[other] { $count } items
}
```

```lua
local t = tango.load_catalog("./locales", require("@bn-common").t)
local label = t("items", {count = 3})
local folder = t("folder-edit-folder")
local language = tango.locale()
local direction = tango.text_direction() -- "ltr", "rtl" or "ttb"
```

`tango.load_catalog(path, fallback?)` returns a typed translator `t(key, args?)`, bound to
that directory and the host's active locale. It accepts only package-local
relative paths (`./` and `../`), resolved from the loading module. Absolute paths,
traversal out of the package, and cross-package catalog paths are rejected.
Missing directories are errors.

The optional fallback is another translator function, usually imported from a
dependency. Calls search the local catalog's negotiated locales, including its
English fallback, before delegating a missing key or attribute with the original
arguments. Local messages take precedence; formatting errors are reported rather
than hidden by a fallback. A missing key at the end produces `⟦key⟧`. Libraries
can chain their own imported translators and export the resulting `t`, so callers
use one function at any dependency depth. Fallback output has the same 16 KiB
limit, and nested fallback calls are bounded to 32.

Supporting packages export their translator through ordinary Luau modules:

```lua
-- bn-common/init.luau
local t = tango.load_catalog("./locales")
return {t = t}

-- Consumer with its own overrides and labels:
local t = tango.load_catalog("./locales", require("@bn-common").t)
```

The provider owns its catalog layout, fallback and any translation wrapper it
exports. Consumers import `t` without knowing the provider's directory names.
A package can also return a translator from a submodule or re-export one from
its own dependency. These are normal typed `require` imports and use the same
exact-version dependency resolution and module sandbox. Passing the function
between modules does not change its catalog or locale.

The translator supports Fluent interpolation, plural/select expressions,
terms, message references, attributes (`"message.tooltip"`), and `NUMBER`.
Arguments are a typed string-keyed map of strings or numbers, decoded with Serde.
Locale negotiation tries the requested locale and related regional/script variants,
then `en-US`. Fallback happens per message or attribute. Missing keys render as
`⟦key⟧`; reported formatting errors, including missing arguments/references and
cycles, fail the operation. The checker checks the argument types and every FTL
file is parsed on load; dynamically constructed message keys are resolved at runtime.
Fluent's current `NUMBER` implementation is not a complete locale-aware number/date
formatting API; `DATETIME` is not provided.

`bn-common` exports the native BN editor's existing translations in all
eleven languages. Packages and extensions import its `t` for shared wording and
load their own catalogs for additional strings. ROM-derived names and descriptions
retain the ROM's language. The script host contains no BN translation keys.

Hosts call `Profile::with_locale(language)` before opening a document, and
`Document::set_locale(language)` when the app language changes. Refreshing a locale
rebuilds labels and diagnostics while preserving document bytes, draft inputs,
undo/redo, immutable inputs and saved state. A failed refresh leaves the old view
and locale intact. Keep action IDs, selection values and view-state keys independent
of translated text; scripts can use `text_direction()` to choose layout. Automatic
layout mirroring and full RTL visual parity have not yet been verified.

`t()` returns a formatted string. Validation diagnostics update because a locale
refresh reruns validation. For a user-facing failure that may outlive its operation,
use `tango.fail(t, key, args)` instead of formatting an assertion message:

```luau
if not supported then
    return tango.fail(t, "unsupported-save", {version = version})
end
```

This raises an error retaining the catalog chain, key and copied arguments. It
never returns; the explicit `return` also lets Luau narrow optional values after
the guard. It does not keep a Lua closure, document, ROM or VM alive. The translator must come
from `load_catalog`, with at most 32 fallbacks that also come from `load_catalog`;
imported translators work without changing their catalog ownership. Arbitrary
Lua fallback functions remain supported for ordinary `t()` calls, but cannot be
retained by `fail`. Translation arguments have the same bounds and Serde decoding
as ordinary translations.

Hosts retain the `tango_script::Error` and call `error.localized(&locale)` at
display time. The embedded and development editors preserve this error through
the Rust boundary. Language changes keep an embedded edit error visible and
retranslate it without repeating the failed edit. Tracebacks remain unchanged.
If formatting the new translation fails, the original message remains available.
Plain `assert`/`error` strings and already-formatted interpolation arguments do
not retranslate; use plain assertions for internal invariants and pass raw values
to localized failures. Returning `{t("key")}` from `validate` remains supported:
those messages are regenerated on locale changes.

The development `edit` and `test` commands accept `--locale ja-JP`. `tango package edit`
defaults to Tango's configured language; the standalone example and headless tests
default to `en-US`. BN6's additional strings have English and Japanese catalogs;
other locales fall back to English for those keys. Shared editor strings use
`bn-common`'s eleven catalogs.

Catalogs are capped at 64 locales, with 2 MiB of FTL per package and 256 KiB
per FTL file. Up to 64 compiled directories are cached per package; eviction
does not invalidate returned translators. Catalog loads charge a
16 KiB allowance plus the package's FTL byte count against the host work budget,
including cache hits.
Calls accept at most 32 arguments / 8 KiB and produce at most 16 KiB of text, charging
that allowance against the host work budget. Numbers must be finite and within
±(2^53−1); literals support at most 18 fractional digits and `NUMBER` precision
options must be integers from 0 to 18. Fluent also bounds reference expansion.
These limits apply before formatting can allocate unbounded precision outside the Luau VM.

## Host boundaries

Packages have no ambient filesystem, networking, wall clock, external code loading,
coroutines, or random generator. VM operations have a 128 MiB allocation limit
and four million interrupt safepoints; native file/BPS work has a separate
128 MiB byte budget shared with input reads. Host buffers are capped at 32 MiB,
documents at 8 MiB, immutable inputs at 16 slots / 512 MiB total,
file snapshots at 64 MiB and 4096 files per package, and source at 256 KiB per
module / 2 MiB per package. Directory traversal is limited to 32 levels.
The host bounds UI depth,
node count, strings, choices, view state, and undo history. BPS validates CRCs,
lengths, relative offsets, and overlapping copies before accepting an output.

These are implementation limits, not a security audit or a hard wall-time bound.
Before creating a VM or frontend, the host initializes upstream `luau-analyze`'s
supported Analysis flags once per process, retaining mlua's VM/compiler defaults.
It selects the new
type solver explicitly. The type solver has a five-second limit per module; parsing and
compilation do not yet have cancellation or a separate process time limit. The native editor currently executes callbacks on its
UI thread; worker execution is needed before a public untrusted-package catalog.

## Completing the migration

BN6's Rust game, data-view, and editor crates have been removed. Its desktop
picker, save templates, editor, single-player launch, gamemodes, and telemetry
use the bundled package. Steam archive discovery retains unregistered ROMs for
package detection. Legacy unpatched BN6 SIO replays now import through the gamemode
modules; legacy patch import remains unfinished; the browser does not yet execute package gamemodes. Other games'
native crates remain until their packages take over.

The destination is a Rust core with no Battle Network-specific code. Rust keeps
the shell, emulator adapters, generic rollback, transport/matchmaking, storage,
and a capability-based script host. Packages own all game interpretation.

1. Replace static `GameRef` registration with installed package profiles and
   scripted ROM identification, save discovery, metadata, and asset decoding.
   BNLC discovery should be a package recipe over generic host discovery APIs.
2. Extend the generic emulator capabilities and port the remaining games'
   priming, input marshaling and lifecycle to gamemodes. The GBA hook adapter
   and BN6 startup/lifecycle port are implemented and available through desktop
   session selection. Changing game state stays in emulator memory/registers;
   gamemodes have no separate state buffer. Training scripting remains deferred.
3. Replace BN-shaped telemetry and training/editor interfaces with typed,
   package-owned data and UI. Port the remaining BN editors, including graphics
   and specialized interaction, without adding BN-specific Rust widgets.
4. Move library selection, installation, downloads, updates, and lobby/replay
   negotiation to the same package system. Pin package contents, transformed
   ROM digest, engine ABI, and simulation script ABI in a replay/netplay profile;
   freeze it for the session. Package names or claimed compatibility groups
   alone are insufficient. Editor-only extensions need a distinct capability
   domain so cosmetic changes do not accidentally become simulation changes.
5. Import legacy `.tangopatch` archives into extension packages: each BPS becomes
   an asset and existing game/editor overrides become Luau. Keep the old reader
   only for migration and old replay import, then remove the old catalog/schema
   once imports are covered. Remove each per-game Rust implementation as its
   package is implemented, rather than waiting for every game to migrate.

There should be one permanent package installation/update path. The old patch
path is a transition boundary, not another supported extension architecture.

## Checks

```sh
cargo test --release --locked -p tango-script
cargo check --release --locked --no-default-features -p tango-library --target wasm32-unknown-unknown
```

The interaction benchmark replays a package-authored JSON trace against local
ROM/save inputs and reports median/p95 dispatch times. It never writes the save:

```sh
cargo bench --profile release --locked -p tango-script --bench editor -- \
  ../packages bn6 /path/to/save.sav /path/to/rom.gba ../packages/bn6/editor/benchmark.json
```

Paths passed to Cargo's benchmark are relative to `tango-script/`. Bytecode and
native code are cached within each immutable package graph, while Lua environments and upvalues
are recreated for every operation. Unchanged document bytes reuse diagnostics;
unchanged bytes and view state reuse the view without dropping returned effects.

The integration suite discovers every directory under `packages/`, strictly
checks it, resolves its dependencies, and runs its Luau tests. Game fixture assertions
stay in Luau; Rust tests exercise generic transactions, composition, quotas,
container validation, and storage installation.
