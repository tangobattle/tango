# BN6 ROM decoder comparison

The Luau decoder in `packages/bn6/editor/rom/init.luau` was compared against
the retired native BN6 ROM decoder (available in Git history) using local, unmodified revision-0
US/JP Gregar/Falzar ROMs and the corresponding bundled raw save fixtures.

All 411 chip records in each ROM matched: names, descriptions, available codes,
element, class, MB, attack power, library ordering, and the legal-chip set.
Every byte of each 16×16 icon and 56×48 artwork image matched, as did all eleven
16×16 element icons per ROM. That covers 1,644 records and 3,332 images across
the four variants. Neither ROMs nor their extracted graphics are bundled.

The comparison runs in batches of at most sixteen chips, within the normal
runtime execution quota. `packages/bn6/editor/rom/test.luau` consumes a native
reference as an explicitly supplied input:

```sh
cargo run --release --locked --bin tango -- package test \
  --rom local-game.gba --input reference=local-reference.bin \
  packages/gba packages/editor-common packages/bn-common packages/bn6
```

Reference files contain little-endian `u32` values. Strings are a byte length
followed by UTF-8 bytes; `0xffffffff` represents an absent string. Images are
width, height, byte length, then RGBA bytes. The sequence is:

1. The strings `tango.bn6-rom-reference-v1` and the four-byte cartridge code.
2. A record count from 1 to 16.
3. For each record: chip ID, name, description, code count and numeric codes
   (`A = 0`, `* = 26`), element, class string, MB, attack, ordering, legal flag
   (`0` or `1`), icon image, artwork image.
4. The eleven element images, in ID order.

Classes are `standard`, `mega`, `giga`, `program_advance`, or `none`.
The script checks complete consumption of the reference and the ROM variant.
Generate reference files from the historical native decoder with the same effective ROM
and raw save bytes, covering chip IDs 0 through 410 exactly once per variant.

Without a reference input, the regular package tests exercise synthetic pointer
tables, message commands, WRAM descriptions, palette transparency and color
conversion, tile positioning, malformed graphics, and compressed data, alongside
the existing four-region save tests. Generic Rust tests check immutable input
identity, bounds, execution budgets, and preservation through undo/redo.

The native `Document` host was also exercised with each of the four ROM/save
pairs: all thirty folder icons survived Serde decoding, original save bytes
roundtripped exactly, and an edit followed by undo/redo encoded correctly.
This establishes ROM data behavior, not embedded editor parity. Folder interaction and legality presentation,
NaviCust and patch-card editing, localized layout parity, host effects, and the full embedded layout
and interactions still require the checks in [EDITOR_PARITY.md](EDITOR_PARITY.md).

## Parts and save-model coverage

The same four variants also matched all 188 NaviCust part records and their
compressed/uncompressed bitmaps per ROM, the available Navi emblems, and all
117 Japanese patch-card records and effect lists. This covers 752 part records,
1,504 bitmaps, 28 available emblems, and 234 patch-card records.

`bn6/editor/save/model.luau` was compared against the native save views after 173 operations
per variant, covering folders, chip slots and pack counts, regular/tag conflicts,
all twelve Navi identities, capability changes, NaviCust placement records,
patch cards, invalid indexes, and both anticheat mirrors. All changed bytes and
getter snapshots matched. The four `editor/save/fixtures/*-model.bin` files contain these
native references, derived solely from the bundled raw saves, without ROM data.
The baseline includes the native regional-layout normalization. Separate script
checks preserve unrelated SRAM and the Japanese trailing gap, and reject
non-integral, non-finite, overflowing, or out-of-capacity mutation requests.

The regular package suite checks every mutation byte, with full getter snapshots
at each Navi transition and periodic checkpoints to stay within its execution
quota. To check every snapshot, run each reference separately:

```sh
cargo run --release --locked --bin tango -- package test \
  --input reference.model=packages/bn6/editor/save/fixtures/g_us-model.bin \
  packages/gba packages/editor-common packages/bn-common packages/bn6
```

Repeat for `f_us`, `g_jp`, and `f_jp`. Additional local ROM-backed references
covered 5,734 operations in 68 bounded groups. Each operation matched the native
save bytes, getters, and derived HP, buster levels, and folder limits. Cases
included every NaviCust part in centered and clipped placements with rotations
and compression, every Japanese patch card enabled and disabled, all Navi
identities, and mixed builds. These references stay outside the package and
use the same `reference.model` input together with `--rom`.

Save-model references use little-endian `u32` values and length-prefixed UTF-8
strings. Their format is:

1. The string `tango.bn6-save-reference-v1`, variant name, ROM-required flag,
   raw baseline length and bytes, then operation count.
2. Per operation: name, argument count and arguments, accepted flag, changed-byte
   count and `(offset, byte)` pairs, then getter count and values.

Getter order is defined by `editor/save/model_test.luau`'s `snapshot`; absent chip and
placement fields use `0xffffffff`. ROM-backed snapshots append derived stats.
All offsets address the original regional raw save. These checks establish the
model behavior exercised above; interactive editing controls still require
their own parity coverage.

## Build rules and folder edits

`bn-common/folder/rules.luau` now owns the shared folder usage, class/copy
limits, code availability, Regular/Tag memory checks, and warning grouping.
`bn6/editor/build/init.luau` combines these with the current Navi's derived limits,
ordered patch-card memory accounting and NaviCust materialization checks. The
package's `validate` callback uses this report when a matching ROM is supplied.
Without a ROM it retains the simpler save-format checks.

The structured findings and exact localized warning strings matched the native
dataview and UI warning provider in 209 comparisons: 25 cases for all four
variants in English and Japanese, plus a mixed invalid build in each of the
other nine shipped languages. Cases include empty/incomplete folders, illegal
IDs and codes, copy/class limits, Regular/Tag memory, corrupted NaviCust grids,
patch-card overflow, disabled/unknown cards, and every Link Navi. The shared
library also tests Navi/Dark class handling and order-independent attribution
with synthetic data, without requiring a ROM.

Native document probes for all four variants verified matching English/Japanese
diagnostics, blocking invalid saves, repairing a folder, restoring errors on
undo, and clearing them on redo. The original valid-save probes still pass
exact byte roundtrips and thirty image nodes.

`bn-common/folder/edit.luau` and the BN6 `editor/folder/init.luau` adapter matched
64 native edit operations per variant, including insertion at the top, first-gap
consumption, removal/compaction, ordered moves, Regular/Tag remapping and conflict
checks, clearing, full-folder no-ops, direct slot writes, and memory guards.
The four `editor/folder/fixtures/*-folder.bin` files preserve these native mutation references;
they contain save bytes and edit data, without extracted ROM assets. The initial
Regular memory is lowered to exercise its guard. Run each with its matching ROM:

```sh
cargo run --release --locked --bin tango -- package test \
  --rom local-game.gba --input reference.folder=packages/bn6/editor/folder/fixtures/g_us-folder.bin \
  packages/gba packages/editor-common packages/bn-common packages/bn6
```

The folder reference format uses the same length-prefixed strings and `u32`
encoding: `tango.bn6-folder-reference-v1`, variant name, raw baseline length and
bytes, operation count, then each operation's name, arguments, and changed-byte
pairs. See `editor/folder/test.luau`. Shared tests cover pending single-Tag
selection, drop-to-last-filled behavior, removal, and remapping independently
from the save's complete Tag pair.

Local build references use the `reference.build` input and an explicit matching
`--locale`. Their format is `tango.bn6-build-reference-v1`, case name, locale,
raw baseline, a tab-separated structured report, then a count and strings for
the grouped warnings. `editor/build/test.luau` checks both and verifies that validation
leaves every input byte unchanged. These references contain ROM-derived names
and remain local.

The shared folder presentation is now in `bn-common/folder/ui/`, with
the BN6 adapter in `editor/folder/ui.luau`. A local US Gregar document check
opened the complete library (17,434 UI nodes, including tooltip contents),
preserved an exact SRAM round trip, and checked clear, undo, redo, and refusal
to save an incomplete folder. This exercises the actual script-produced tree
and host transactions; it does not establish rendered pixel or input parity.

Local folder presentation checks render the native and scripted views from the
same US Gregar save and ROM using Iced's software renderer, Tango's fonts, and
matching theme/locale/viewport settings. Forty comparisons match exactly:

- English and Japanese UI, light and dark themes.
- Initial layout at 1200×600 and 900×480.
- At 1200×600: grip/chip/library hover, Regular/Clear clicks, library scrolling,
  an active drag, and a completed drop.

The comparisons check pixels, cursor shapes, and mapped actions. They exposed
and verified fixes for shrinking disabled library rows, a missing grab cursor,
and different drag styling. Cursor hints are generic SDK fields; the drag style
is shared by both renderers in `tango-ui`. The development host now gives the
script a bounded viewport so its two panes own their scrolling.

An English US Gregar edit sequence adds 44 exact comparisons (light/dark,
idle/library hover at 1200×600) across eleven checkpoints: Regular selection,
pending Tag, Tag pair, reorder, remove, search, sort, clear, empty search results,
cleared search, and add. Each checkpoint also compares native and scripted
validation messages; valid saves match every encoded byte, while invalid folder
states reject saving. Pending Tag state is included even before a complete pair
can be stored in SRAM.

These comparisons do not cover the full folder UI or embedded visual/interaction
parity. Those remain subject to [EDITOR_PARITY.md](EDITOR_PARITY.md).

## Read-only folder presentation

`bn-common/folder/display.luau` groups chips by ID and code in first-appearance
order, retaining represented slots and their REG/TAG flags. `ui/viewer.luau`
renders counts, artwork, metadata badges, class stripes, statistics and unique
legality reasons. Ungrouped display skips empty slots; grouped display retains
one empty-slot entry. The BN6 adapter supplies the same ROM catalog and build
limits as its editable folder.

The development host now accepts `package edit --read-only`. It supplies the
script's `EditorContext` and allows view-state updates, including the grouping
checkbox, while rejecting document mutations atomically. This is a host
capability, not a script-controlled flag. Generic tests attempt to override it
through callback arguments and verify that bytes, view state and history remain
unchanged on rejection. Checkbox metrics and actual toggle events are compared
against the native control in both themes.

120 local folder comparisons match pixels, cursors and emitted actions, using
the native and scripted scrollable viewers with the same fonts:

- US Gregar with English UI: populated, partially emptied, fully emptied and
  invalid folders. The populated case includes three identical chips carrying
  REG, TAG1 and TAG2; the invalid case includes excess copies, an unavailable
  code, an unknown chip ID and an empty slot.
- Japanese Gregar with Japanese UI: the populated case, including Japanese
  ROM names and descriptions.
- Each case: grouped/ungrouped, light/dark, 900×480/380×300, and idle/hover/scroll.

Ten additional US Gregar comparisons match the read-only identity strip with an
empty host actions slot: both themes, idle/hover/click at 1200×300 and idle at
500×260/240×260. Clicking emits no edit action. These checks do not establish the
complete embedded viewing shell or cover the remaining locales, variants and
patches. The development read-only view also exposes NaviCust and patch-card
sections when the save supports them; clipboard effects and embedding are still
pending.

The NaviCust viewer shares the editor's scripted paint commands, installed-part
badges and cell tooltips. It deliberately paints the save's stored materialized
grid; the editable board recomputes materialization. A generic rasterized canvas
matches the native high-resolution image and corner mask, while a separate
fixed-coordinate canvas supplies the script-owned hover outline. The host does
not resolve parts, colors, grids, or placement rules.

180 local section comparisons match the native scrollable viewer and corresponding
scripted node, with the same fonts and inputs. They include light/dark themes,
900×600/380×300 viewports, idle, board/card hover, badge hover, clicks and scrolling.
Pointer observations are dispatched through the read-only document before the
scripted view redraws; no save edits or undo entries are allowed. These checks
cover:

- US Gregar NaviCust: two installed solid/plus parts and a save
  whose placements remain installed while its materialization is cleared.
- US Falzar NaviCust: an empty grid.
- Japanese Falzar NaviCust with Japanese UI: populated solid/plus parts.
- Japanese Gregar patch cards with English UI: populated rows including a
  disabled card, empty rows, an over-budget list, and mixed enabled/disabled cards.
- Japanese Falzar patch cards with Japanese UI: populated rows.

Read-only tab navigation also preserves folder grouping, diagnostics and document
state. Sixteen additional comparisons recheck the initial editable NaviCust and
patch-card panes after extracting shared badges/tooltips and correcting name-cell
containment for narrow layouts.

Generic renderer tests separately cover transparent rasterization, corner masks,
cache reuse/invalidation, image-equivalent fitting and filtering, and fixed or
letterboxed pointer coordinates. Host tests reject excessive source dimensions,
combined image memory and rasterization work. These are component comparisons;
the complete viewing shell, transitions, actions and clipboard outputs remain
outside this coverage.

## Patch-card editor presentation

The shared editor is in `bn-common/patch_cards/`, with its presentation in `ui/`
and the BN6 adapter in `bn6/editor/patch_cards/`. The development view offers
Folder and Patch Cards navigation when the save supports cards. This navigation
is temporary; it does not establish parity with the native editor shell.

Local Japanese Gregar comparisons use the same software renderer and fonts as
the folder checks, with English UI text and Japanese ROM-derived card effects.
Eight initial-state comparisons match pixels, cursor shapes, and mapped actions:
light/dark themes, idle at 1200×600 and 900×480, and library hover/scroll at 1200×600.

A sequence seeded with an existing disabled card adds 52 exact comparisons
(light/dark, idle/library hover at 1200×600) across thirteen checkpoints: five
additions reaching the memory limit, an over-budget addition, reorder, remove,
sort, filter, clear, empty search results, and cleared search. At every checkpoint,
native and scripted validation messages match. Valid saves match every encoded
byte; invalid over-budget builds reject saving. Disabled cards retain their
state, strikethrough, muted labels, and gray effect badges. Over-budget library
rows remain actionable and use the native danger colors.

This covers sixty component comparisons. It does not establish parity for the
complete embedded editor, other ROM variants or UI languages, patch-card pointer
click/drag sequences, read-only presentation, or clipboard exports. ROMs and
ROM-derived captures remain local inputs and are not bundled.

## NaviCust editor presentation

The component lives in `bn-common/navicust/`, with `ui/` owning board drawing,
thumbnails and controls, and `bn6/editor/navicust/` supplying save/ROM adapters.
The development editor exposes it for MegaMan saves. The package owns all
part geometry, placement legality, rotation/compression, pickup offsets, and
the nine-copy limit. Rust supplies general canvas strokes, image opacity,
nested controls, and pointer capture filters.

Eight local US Gregar comparisons with English UI match pixels, cursor shapes,
and mapped actions: light/dark themes, initial layout at 1200×600 and 900×480,
and library hover/scroll at 1200×600. These exposed missing row expansion,
rectangle rasterization differences, and f32 corner rounding.

An edit sequence adds sixty exact full-component comparisons (light/dark,
idle/library hover at 1200×600) across fifteen checkpoints: clear, pick, rotate,
uncompress, place, pick up an installed part, wheel rotation, replace, filter,
sort, clear again, pick again, discard, no search results, and cleared search.
Every checkpoint matches native validation and all encoded SRAM bytes.

An additional 120 isolated-board comparisons check the center, blocked corner,
clipped edge, and outside cursor positions in both themes at each checkpoint.
Pixels and cursor shapes match, covering legal/illegal placement ghosts,
rotated and uncompressed previews, and installed-part hover outlines. The board
is placed at the viewport origin for these checks so its whole surface is
visible independently of the software renderer's clipping in the pane layout.

Eight further comparisons exercise actual clicks on the palette row, its nested
rotate and compression buttons, and Clear, in both themes. Each emits exactly
the corresponding native action, with matching pixels and cursor shapes.
This brings the local NaviCust component coverage to 196 comparisons, all for
US Gregar with English UI; it is not a claim of complete embedded-editor parity.

Shared Luau tests cover collision rejection, blocked corners, clipped footprints,
parts entirely in the outer ring, retained and rotated grab offsets, compression
recentering, slot compaction, copy limits, materialization ordering, and Unicode
filtering. Generic host tests cover nested button action routing, disabled
subtrees, image opacity, and selective pointer capture. These do not replace
the remaining region/variant, patched-ROM, full pointer/keyboard, read-only,
clipboard, and embedded-shell checks required by [EDITOR_PARITY.md](EDITOR_PARITY.md).

## Navi identity and selection

Shared presentation and emblem processing live in `bn-common/navi/`; the BN6
adapter and its state-transition tests live in `bn6/editor/navi/`. The identity
strip shows the current Navi and ROM/build-derived HP and buster statistics.
The picker uses the ROM's roster order, cropped emblems, accent-colored plates,
selected glow, and dimmed unselected entries. Selecting a Navi closes the picker
and updates which sections are available, retaining the previous tab preference,
pending Tag selection and held NaviCust part.

Forty initial-state comparisons match pixels, cursor shapes, and mapped actions:
US Gregar with English UI and Japanese Gregar with Japanese UI, both themes,
1200×300 with idle/hover/click states and 500×260/240×260 with idle cursors. The
identity strip is compared with an empty actions slot; the embedded Save/Cancel
and Play cluster is not covered. These checks exposed differences in overflowing
text culling, fixed by using the same general row/column widgets as native Tango
and avoiding redundant layout wrappers. Generic presentation tests retain this
case along with explicit clipping, wrapping modes, theme tints and shadows.

For each of US/JP Gregar/Falzar, an English-UI sequence selects all seven available
Navis and then returns to MegaMan. All 32 checkpoints match native validation,
all encoded SRAM bytes, picker dismissal and available sections. At each
checkpoint, both the identity strip and reopened picker match native pixels in
light and dark themes at 1200×300: 128 additional component comparisons, for
168 total. The folder, NaviCust and patch-card initial presentation/input cases
were also repeated after the generic layout changes and still match.

ROM-free Luau tests cover emblem cropping, the alpha threshold, saturation and
frequency weighting, invalid/out-of-roster selection, and preservation of editor
state across every regional roster. Equal accent scores use the first pixel's
color for deterministic results; the native implementation's hash iteration has
unspecified tie order. The checked ROM emblems match the native accents.

The complete embedded strip, read-only mode, transitions, keyboard/focus behavior,
patched rosters and remaining locales still require the comparisons in
[EDITOR_PARITY.md](EDITOR_PARITY.md). ROMs and screenshots remain local.

## One-based package indices

Luau model and collection APIs now use 1-based positions, including folder and
chip slots, Regular/Tag selections, NaviCust and card slots, pack variants,
message-table entries, list drag events and row styles. Encoded game IDs, byte
offsets and drawing coordinates retain their original values. Native reference
files are unchanged; their test readers translate Rust positions at the boundary.
The four bundled model fixtures still compare every mutation byte and their
sampled getter snapshots. Added boundaries check index zero rejection and the
last records directly against their SRAM addresses.
The ROM-backed part, card and Navi metadata references also pass again for all
four US/JP Gregar/Falzar variants after message-table positions became 1-based.

After the conversion, forty native folder presentation comparisons still match
pixels, cursor shapes and emitted actions: US Gregar with English/Japanese UI,
both themes, 1200×600 and 900×480, including scrolling, Regular/Clear clicks,
dragging and dropping. Script drag positions now arrive directly as Lua table
indices, and generic renderer tests exercise both first-to-last and last-to-first
drags. These remain component checks; see the embedding gate above.
