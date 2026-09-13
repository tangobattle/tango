# Embedded editor migration checks

The scripted editor that replaces the editor inside Tango must preserve its
capabilities, save compatibility, and familiar layout. Exact visual matching is
not required when a simpler implementation serves the same interaction. Do not
add host complexity merely to reproduce every native frame. Record intentional
differences and keep checking functional behavior. The `tango package edit`
window is a development harness; its extra toolbar is not the embedded layout.

Parity must be achieved through the [general scripted UI toolkit](UI_ARCHITECTURE.md).
It must remain possible to implement unrelated games' editors with the same
host capabilities; the reference BN layout is owned by its Luau package.

Do not switch a game's normal `SaveEditor` registration, Play selection, replay
panel, or opponent panel to the package implementation until that game's
functional and embedding checks pass. Do not make an incomplete scripted editor
the default or silently
drop features that are difficult to express with the initial node API.

## Reference implementation

The reference is the checked-in editor under
[`tango-gamesupport-common-ui/src/editor`](../tango-gamesupport-common-ui/src/editor),
plus the applicable game's `-ui` implementation. The embedding contract is
[`tango-gamesupport/src/save_editor.rs`](../tango-gamesupport/src/save_editor.rs).
Use the same effective ROM, save bytes, patch/profile, locale, theme, UI scale,
viewport size, and animation phase for both implementations.

The replacement should render through the same game-neutral `tango-ui` widgets,
styles, typography, animation, and interaction primitives. Extract reusable
presentation primitives there when necessary. Do not expose a Rust `BNFolder`,
`NaviCust`, chip rule, or save-offset helper to Luau to obtain parity. Scripts
must supply game-specific models, layout choices, graphics, labels, validation,
state transitions, hit-testing rules, and edits. Generic lists, images, grids,
drag events, tooltips, and clipboard effects may be provided by Rust.

The generic SDK includes layouts, images, inputs, lists, and canvas interactions.
Expand the host surface as the actual editor is ported; do not replace rich
controls with text fields to fit the available primitives.

## Coverage required before embedding

| Area | Required observable behavior | Reference |
| --- | --- | --- |
| Embedding | Play, replay, local/remote opponent panels; editable/read-only modes; enabled/disabled/absent Play; inline/external actions; no file dialogs inside an embed | `save_editor.rs`, `editor/shell.rs` |
| Whole-save session | Existing Edit → Save/Cancel lifecycle; edits across all sections staged together; Cancel restores the original; failed writes preserve edits; session SRAM has a valid checksum without committing to disk | `editor/shell.rs`, `view/state.rs` |
| Shell | Identity/Navi strip and statistics; capability-dependent tabs; selected tab and scroll position; overflow fades; copy/grouping controls; entrance and edit transitions; streamer cover/reveal behavior | `view/mod.rs`, `view/navi.rs`, `view/cover.rs` |
| Folder viewer | Real chip names, artwork, elements, codes, badges, counts, order, grouped/ungrouped display, legality highlighting, localized reasons | `view/folder.rs` |
| Folder editor | Two-pane library and folder layout; filtering/sorting; add/remove/clear; drag reorder; regular/tag toggles; game-dependent limits and editability; correct updates to slot-dependent metadata | `view/folder.rs`, `view/state.rs` |
| NaviCust | Accurate board/part geometry and palette; pick/place/remove; grab offset; hover/ghost/invalid placement; rotation by controls and wheel; compression; palette filter/sort; clear; derived stats and warnings | `view/navicust/` |
| Navi selection | Same card graphics, names, statistics and picker; changing identity updates editable capabilities and available sections | `view/navi.rs`, game `-ui` |
| Patch/mod cards | Lists and libraries; filtering/sorting; add/remove/clear/reorder and enable state where offered; artwork, effects, memory budget and conflicts; BN4-specific editor behavior | `view/patch_cards.rs`, game `-ui` |
| Other games' sections | Auto Battle Data, BCC program deck/party, BN5DS file and cartridge-cross selectors, and any other currently exposed game-owned controls | `view/abd.rs`, game `-ui` |
| Validation | Same actionable warnings and error locations; same save-blocking decisions; rules use the effective patched ROM; all existing localized messages | `build.rs`, `view/components.rs`, dataview validation |
| Clipboard | Exact text/TSV and HTML with icons, plus image exports where offered; same buttons and copy feedback | `view/folder.rs`, `view/navicust/mod.rs`, `view/patch_cards.rs` |
| Save compatibility | Every supported region/version and file layout; checksums, masks, anticheat mirrors, derived data, unrelated SRAM preservation, patches that change layouts or legality | game `-dataview`, package scripts |
| Presentation | Same theme, fonts, type scale, spacing, borders, colors, disabled/hover/focus states, tooltips, scrolling, clipping, responsive sizing, and keyboard/pointer behavior | `tango-ui`, `view/components.rs` |

The native implementation currently has Save/Cancel as its visible edit-session
controls. The development harness's standalone Save As, Reload, Undo, and Redo
toolbar is not a specification for the embedded editor.

## How to establish parity

For each game, capture reference cases before replacing its registration:

1. Start from the same legal local ROM, save fixture, patch, and embed options.
   ROM contents remain local inputs, never committed to the repository.
2. Record action sequences covering the table above, including invalid actions,
   every editable section, transitions between sections, cancellation, and failed
   saves. Keep game-specific assertions and expected data in the package.
3. Compare encoded save bytes, decoded field values, derived state, legality
   decisions, clipboard outputs, and host events after each sequence. Explicitly
   account for any intentional normalization already performed by the reference.
   A new encoder decoding its own output is necessary but does not prove parity.
4. Capture both renderers at the same viewport, pixel scale, fonts, theme, locale,
   selected section, hover/focus state, scroll position, and settled animation
   phase. Use image diffs to find layout and interaction regressions, not as a
   requirement for zero differing pixels. Include compact
   opponent/replay panels and the full Play editor, long translated labels,
   empty/invalid/full lists, and picker/drag states. Verify that intentional
   visual simplifications preserve the controls and their behavior.
5. Run the same cases with extension packages that change ROM assets, limits,
   or editor behavior. Verify the effective ROM is transformed once and the
   script uses that resulting image for both interpretation and validation.
6. Integrate after functional cases pass and visual differences are reviewed.
   Retire the corresponding native code
   after its package owns the full behavior and existing users retain the same
   experience. Repeat for the remaining games.

## Current status

Pixel comparisons below record migration checkpoints. The responsive Navi
picker/stat layout and simplified transitions intentionally supersede their
exact visual baselines; save and interaction comparisons still apply.

Exact visual parity is not a release requirement. The bundled BN5 and BN6 editors now
attach to normal Play, replay and in-match setup panels for recognized unpatched
ROMs. Unported games and their legacy patches keep native editors; converting
legacy patches for migrated games remains outstanding.
The normal selection constructor has been checked with US and Japanese Gregar
and Falzar ROM/save pairs, English and Japanese UI, editable Play, read-only replay,
and streamer-mode in-match configurations. The checks verify scripted attachment,
control availability, edit/picker/cancel transitions, unchanged baseline bytes,
and package-owned editor state. Separate adapter tests cover failed and successful
save acknowledgments, stale messages, failed permission changes, and combined
scroll/host effects. These are functional embedding checks, not new pixel-parity
claims. BN5 and BN6 game launching now uses package gamemodes.

BN6's initial migration checks covered
strict scripted save decoding/encoding, four regional/variant fixtures, basic
folder mutations, and host transactions. Its save model also covers regular/tag
slots, the chip pack, Navi selection, NaviCust placements, and patch-card mutations
against native reference fixtures. Immutable ROM inputs support scripted chip
text, metadata, icons, artwork, element graphics, NaviCust parts, Navi emblems,
and patch-card effects. BN6's ROM-backed validator now checks folder legality,
patch-card memory, and NaviCust materialization, with grouped localized warnings.
Its shared folder edit applier covers insertion, removal, ordering and Regular/Tag
metadata. The development view now has the two-pane folder editor, search/sort,
add/remove/clear, Regular/Tag controls, drag reorder, and chip/reason tooltips.
Local software-rendered comparisons for a US Gregar save match the native folder
view in English and Japanese, with light/dark themes and two viewport sizes.
They also compare hover, control clicks, scrolling, dragging, and dropping,
including cursor shapes and emitted actions. See [the comparison record](ROM_DATA_PARITY.md).
The Japanese MegaMan development view also has a patch-card editor backed by
shared Luau catalog, mutation, memory-rule, and presentation modules. Sixty local
English-UI comparisons for Japanese Gregar match the native view, including
disabled cards and edits that cross the memory limit. Thirteen checkpoints
match validation and encoded saves (or reject invalid saves). The generic host
supplies text decorations and palette colors used by the scripted card rows.

The MegaMan development view now also includes the scripted NaviCust editor.
Shared Luau modules own materialization, placement legality, pickup offsets,
rotation/compression state, copy limits, palette ordering, board/ghost drawing,
thumbnails, and installed-part badges. Local US Gregar comparisons cover the
initial editor and a fifteen-step edit sequence, including exact save bytes and
validation. These are selected component cases; the remaining game variants,
translations, presentation states, and complete pointer workflows still need
comparison. See [the comparison record](ROM_DATA_PARITY.md).

The scripted Navi strip and picker now cover identity, HP/buster statistics,
emblem accents and glow, selection, and capability-dependent sections. Local
comparisons cover every selectable Navi in US/JP Gregar/Falzar, with exact save
bytes, validation and selected-component pixels in both themes. Initial-state
English and Japanese UI comparisons include narrow layouts, hover and actual
clicks. These do not yet include transitions or patched rosters. Separate
read-only strip comparisons cover an empty host actions slot. The strip now
also contains scripted Edit/Cancel/Save controls driven by a generic host edit
session. The Play control now uses a named host capability, with matching
enabled/disabled button chrome and an absent state. Inline/external placement
controls whether Edit/Save/Cancel and the tab's copy/grouping controls appear.

The read-only folder viewer now groups by chip ID/code in first-appearance order,
combines REG/TAG badges and unique slot issues, and supports ungrouped display.
Local comparisons cover populated, incomplete, empty and invalid folders in
both themes and two viewport sizes, including hover tooltips and scrolling.
Japanese UI/ROM content is also compared. Host tests prove that view-state changes
are allowed while attempted document mutations are rejected atomically. The
development `--read-only` path also exposes capability-dependent NaviCust and
patch-card sections. NaviCust displays the save's cached materialization and
highlights the part under the pointer, with shared tooltips and installed-part
warnings. Patch cards preserve disabled names/effects and ordered memory-limit
warnings. Local comparisons cover these viewers in both themes and wide/narrow
layouts, with empty, populated, malformed/over-budget and Japanese cases.
This does not constitute a complete replay/opponent editor.

Clipboard export is now package-owned. Folder text and HTML preserve grouping,
REG/TAG suffixes, empty/unknown entries and self-contained cropped PNG icons.
NaviCust exports preserve solid/plus TSV ordering and the stored materialization
at native image resolution. Patch-card text omits disabled cards. Local comparisons
cover US/JP Gregar/Falzar, both document modes, grouped/ungrouped folders,
empty and invalid saves, Japanese text and exports after staged edits. The
generic copy control has pixel/input comparisons against the native widget in
both themes, before and after acknowledgment, plus expiry and renderer isolation.
Host tests cover atomic effect validation, resource limits and read-only copies;
they do not write to the OS clipboard. Full embedded placement and the real
clipboard transport still belong to the end-to-end editor gate.

These are partial component checks, not the whole embedded-editor gate.
The host now enforces the whole-document edit lifecycle: begin, cancel to the
last saved baseline, failed-write retry, and successful adoption of encoded bytes.
Save receipts are scoped to the exact attempt and session; all package callbacks
for the post-save view complete before storage receives a write request. Session
snapshots repair format checksums without committing or hiding build warnings.
Generic lifecycle tests cover those boundaries and package-owned view-state
reset. Control comparisons cover English/Japanese, both themes, invalid/disabled
Save and pending writes. Local BN6 comparisons additionally check the complete
identity/action strip and native snapshots across staged edits, cancellation,
failed-write retry and successful Save. US Gregar/English and JP Falzar/Japanese
each pass 144 settled header comparisons (two themes, two widths and four pointer
states across nine lifecycle checkpoints). These omit Play and transition frames.
The settled tab strip also matches native pixels, input and cursors across
1,620 local cases: US Gregar/English and JP Falzar/Japanese; viewing, editing and
invalid folders; all available tabs; three horizontal scroll positions; both
themes; widths 900/380/250; and six hover/click states. These comparisons exposed
and fixed fill sizing around the contextual controls. Generic tests exercise
gradient fades, warning tooltips, scroll resets and renderer ID isolation. These
are strip comparisons, not complete embedded screenshots.

Named host action capabilities and inline/external control placement now have
576 additional settled header comparisons: US Gregar/English and JP
Falzar/Japanese, before Edit, during Edit and after Cancel; both placement modes;
absent/disabled/enabled Play; both themes; widths 900/380; and four pointer states.
The host separately verifies revocation, forged context tables, combined
edit/action transactions and pending writes. These checks do not launch a game.

Script collection indices are now 1-based, including list drag events and model
positions. Native reference adapters translate their recorded Rust positions at
the test boundary; encoded bytes and native rendering remain the specification.
Forty repeated native folder comparisons cover both themes and English/Japanese
UI, including regular selection, scrolling and actual drag/drop after this change.
The streamer cover now has 240 settled native pixel/input/cursor comparisons:
US/JP Gregar/Falzar with English/Japanese UI, read-only and editable sessions,
both themes, sizes 900×600/380×480/250×240, and five pointer states. Regional
logos are package files decoded through bounded `tango.decode_png`; the layout
uses ordinary nodes from `editor-common/cover`. Review preserves document bytes
and history, survives locale/preference/edit transitions, and resets on reopen.
Hidden editor actions are rejected while covered. These checks compare the
cover gate directly, without the native entrance animation or app shell.

Entrance motion now uses generic node descriptors and renderer-owned clocks.
Local comparisons sample the complete editor view at 0/40/80/120/159/200 ms,
in both themes at 900×600 and 380×480: cover, Review, forward/backward tab
changes, a repeated active-tab click, entering Edit, changing tabs while editing,
and Cancel. The reference is the native view with an explicit clock parameter;
its independent edit-button morph is held settled to isolate entrance behavior.
All 384 frame comparisons match for US Gregar/English and JP Falzar/Japanese.
Generic tests also
compare 72 animated button pixel/input/cursor cases, delayed timing, restart and
eviction behavior, renderer isolation, bounded descriptors, and frame scheduling.

The complete-view checks exposed two narrow-layout differences beyond motion:
NaviCust now clips its fixed-scale board, and fill-width text uses the native
text widget's dimensions and automatic direction. Generic regressions compare
plain and decorated English/Japanese text at zero and positive remaining widths.

Picker open/close and selection reuse the existing vertical entrance.
Only the current region is built; rapid
toggles restart its animation. The next tab or session change resumes the usual
body entrance. Header Edit/Save/Cancel controls switch immediately instead of
retaining outgoing controls for a fade-through morph. These intentional
differences do not require a new host transition or conditional-node API.
Picker roster groups now wrap their cards inside the available width, and
header statistics wrap alongside the save controls. This keeps narrow panels
usable instead of reproducing the native row overflow.
Full-view smoke checks for US Gregar/English and JP Falzar/Japanese render 144
arrival/settled frames across both themes and 900×600/380×480 panels. They cover
Edit, picker open/dismiss/reopen, changing Navi, returning to MegaMan, and Cancel;
check view-only byte preservation, capability-dependent tabs, stale-action
rejection, and restoration of the original save. Pointer scans with scrolling
reach all seven roster cards in each narrow panel. These are functional and
rendering checks, not pixel equality claims or app-embedding verification.
Later embedded-adapter and launch checks are recorded below.

The native embedded editor was the reference for these comparisons. `.tangopkg` installation and the upstream Luau runtime can be developed and tested
independently of that replacement.


## Retiring native BN6 support

The BN6 game, data-view, and editor crates have been removed. Save templates
now live under `packages/bn6/editor/save/templates/`; the editor exposes their
localized labels and encoded SRAM through the generic template API.

Release checks with local US/JP Gregar/Falzar fixtures confirmed all four
package-created saves match the historical native templates byte for byte.
Each boots through the generic solo backend and opens in the embedded editor
with editable, read-only, streamer, English, and Japanese contexts. The checks
also exercise edit/cancel without writing the source saves. Run them with:

```sh
TANGO_TEST_ROM=/path/to/rom.gba TANGO_TEST_SAVE=/path/to/default.sav \
  cargo test --release --locked --bin tango --all-features \
  package::editor::embedded::tests::bundled_ -- --ignored --nocapture
```

Package lobby, live play, recording, replay, and seeking were also checked
with an original ROM and a copy with modified unused padding. Neither is in
the native registry after removal. These checks cover new package recordings;
legacy BN6 patches remain migration work. Unpatched legacy SIO replay import
is now handled by the package’s gamemodes.


The legacy replay import was compared against the retired native hooks for
both player-perspective recordings of a completed 2,569-frame match. Every
emulator snapshot digest matched, including after capture restoration in a
fresh playback instance. Analysis reported completed rounds, both embedded save
previews re-encoded the original SRAM, and source files were unchanged. Tests
with all four regional cartridges also cover each of the three mode mappings
and rejection of unsupported simulation/engine revisions, subtype codes,
patches, and modified ROM bytes. The historical native hook source and generated
reference digests stay outside the workspace; no BN6 Rust implementation is
linked into Tango.

## BN5 migration

`packages/bn5` exports the single/triple gamemodes for revision-zero US and
Japanese Protoman/Colonel, telemetry, and an editor with folders, NaviCust,
Patch Cards, Auto Battle Data, the team Navi roster, clipboard controls,
Light/Dark templates, build validation, and streamer cover/reveal. The native
editor exposes alignment through save templates, not direct karma controls;
the script follows that behavior. The native game, data-view and editor crates
and their registry/features have been removed. BN5DS remains a separate native
implementation. Live lobby handoff and recording remain unverified as described
below; legacy patch conversion and browser package integration are outstanding.

The gamemodes were compared against the current native BN5 hooks using local
ROMs and the native light/dark saves. Sixteen cases cover both region pairs,
both variant orders, single/triple, and BGM enabled/disabled. Both implementations
prime in 140 ticks for US and 141 for Japanese cartridges. All 360 subsequent
emulator snapshots per case match, including 160 ticks repeated after restoring
a checkpoint: 5,760 compared snapshots and 2,560 restored snapshots. These checks
cover boot and rollback.

Eight additional complete matches cover both region pairs, both variant orders
and single/triple. Light templates have their HP reduced before boot; every
subsequent action is recorded controller input. All 16,996 emulator snapshots
and round/match lifecycle events match the native engine. Each single match
finishes after 1,200 frames; each triple finishes after 3,049 frames and includes
wins for both players. These fixtures do not cover every possible battle outcome.

The native-format recordings from both local perspectives pass the desktop
legacy replay import path: 16 complete replays, with every frame matching the
native digest, fresh playback capture restoration, analysis outcomes, and both
editor save previews. Import requires the exact unpatched cartridge, mGBA engine
revision 2 and native simulation revision 1. Older simulation revisions and legacy
patches remain unsupported by this importer. BN5 and BN6 share cartridge and
legacy metadata checks in `bn-common/gamemode/legacy_replay`; each game supplies
its cartridge identities and simulation revision.

Separate telemetry comparisons cover 8,192 samples across all four cartridges
and both viewpoints, matching native unit selection, HP, positions, custom-screen
flags, chip-use events and round resets. These include invalid/swapped unit
owners and repeated chip IDs. The live desktop lobby check reached the local UDP
socket setup but failed with an OS permission error in the restricted environment;
live handoff and recording remain unverified for BN5.

The save-codec fixtures cover all eight native light/dark templates. Encoded
SRAM CRCs come from the native encoder with its checksum rebuilt; the scripted
codec matches those references, preserves surrounding SRAM, and rejects truncated,
corrupt and unsupported shifted saves. Run the checked-in cases with:

```sh
cargo test --release --locked -p tango-script --test packages
```

The save-model oracle covers 184 operations on each of the eight templates:
folders/Regular/pack counts, both anti-tamper mirrors, NaviCust placement and
materialization, Patch Cards, karma and its mirror, dark HP losses, Auto Battle
Data counts/materialization, Navi changes, and rejected edits. Every operation
compares the entire WRAM mutation and getter snapshot against native output,
including derived HP and folder limits. The checked-in fixtures use synthetic
ROM assets; private ROMs are not needed for those tests.

Local ROM comparisons cover all four cartridges, each with 368 chips, 13 element
icons, 192 NaviCust parts, 111 Patch Cards and seven selectable Navis. Every decoded
field, name/description, bitmap, and RGBA pixel agrees with the native reader.
References generated from private ROMs remain outside the repository.

Shared code lives under `bn-common/gamemode/lifecycle`, `bn-common/telemetry`,
`bn-common/save/codec`, `bn-common/rom`, `bn-common/navicust/materialize` and
`bn-common/navi/active_effects`. BN6 uses those implementations too. Rechecking
176 retained BN6 ROM/model reference batches passed after extraction. This also
fixed a stale zero-based conversion in BN6's active NaviCust effect selection;
materialized cells already contain 1-based placement slots.

`bn-common/auto_battle_data` preserves native ranking, tie-breaking and 42-slot
allocation, with dense optional-ID records for empty slots. It also owns the
six-section deck preview, legal-chip library, Unicode search, stable sorting,
primary/secondary use-count editing, clearing, and localized text export.
Counts are game-owned; BN5 supplies its two save tables. Shared build reporting
and editor session controls now serve BN5 and BN6; their navigation motion
lives in `editor-common/navigation_motion`.

The checked-in `bn5-editor` test fixture runs the exported editor against a
synthetic catalog for all eight saves in English and Japanese. It checks tab
availability, read-only copying, folder edits without Tag Chips, count
normalization, rotated NaviCust placement, card memory overflow and reordering,
Navi changes, retained preferences, streamer reveal, and missing-ROM messaging.
Each case uses a fresh restricted runtime, separate from the model-reference
suite; aggregate test work does not require relaxing normal runtime limits.

Local native comparisons cover all 16 ROM/save/locale combinations: all four
tabs, save creation, text clipboard output, save mutations and warnings after
folder/card/count/Navi edits, NaviCust placement/pickup/wheel rotation, undo/redo,
cancel, and failed/successful save acknowledgments. NaviCust image exports also
match the native image dimensions and every RGBA pixel after placement and
rotation, for all 16 cases. Section captures compare
English/Japanese UI, US/Japanese Protoman, both themes and edit/view modes;
additional captures cover populated sections for all four cartridges. Folder and
Auto Battle Data layouts match, including their populated states. NaviCust
captures agreed, but both renderers clipped most of the board in that early
headless section harness; those captures did not prove full-board layout or
pointer behavior. The complete-panel checks below supersede that limitation.
Reviewed US Patch Card captures have small text-rendering differences. These checks
exercise the real `EditorSession` and renderer. Private ROM-derived captures and
the native oracle remain outside the repository.

The desktop embedded-editor and template/solo tests also pass with all eight
light/dark cartridge/save combinations. They exercise package resolution,
English/Japanese panels, edit/read-only and streamer configurations, picker/cancel
transitions, exact template SRAM, and 180 solo ticks. The normal-panel test now
uses the real game/save pickers and filesystem save scan. It checks package
gamemode selection, unchanged source files, and the absence of native game or
editor state. All eight cases also pass in a build with no native BN5 crates.
The 16 native-format replay comparisons pass in that build too.

Whole-panel software-rendered comparisons cover all four tabs, viewing/editing,
and 1200×640/440×560 viewports for US Protoman with the light template, English
UI and dark theme. Fifteen of 16 pairs match every pixel. The wide Patch Card
editor differs by 181 text pixels; the controls, decoded text, bytes and warnings
agree. Contextual controls must have their entrance motion settled recursively
inside containers before capturing. A separate pointer/keyboard comparison uses
actual Iced events for NaviCust palette selection, rotation, compression,
placement, pickup, wheel rotation, replacement, clearing, and typing a search.
Every edit matches native SRAM and warnings, and the final clear restores the
original save. This covers those gestures, not every possible app interaction.

The clipped-board defect was in Iced's software canvas renderer: it transformed
already-transformed clip bounds twice. The local `iced_tiny_skia` patch fixes
primitive drawing and cached damage bounds. An independent pixel test in
`tango-script-iced/tests/presentation.rs` fails without the fix and passes with
it for cached/uncached translated geometry and both themes. The complete-panel
captures now display the full board at wide sizes; narrow clipping matches the
native fixed-scale layout. The generic presentation suite passes all 26 tests.

The checked-in `bn5-method-extension` fixture also exercises a dependent package
replacing ordinary ROM/editor methods. The existing reader, validator and editor
all see the replacement chip name. No separate override schema or private ROM
is needed. Together with the common method-extension and ROM-transformation
checks, this preserves extension behavior as native support is retired.

## BN4 migration

BN4's package has its save models, ROM readers, effect catalogs, derived stats
and Mod Card legality. It does not yet expose an editor or gamemode and does not
replace the native registration. The Mod Card form, embedded editor, gamemodes
and telemetry still need porting and comparison.

Native references cover twelve templates, with exact encoded CRC32 checks. An
additional 96 inputs cover four save shifts (including the maximum 508 bytes)
and both explicit and ambiguous region detection. The normalized encoding matches
the native result, while preserving surrounding SRAM. The decoded document keeps
its variant and region independently of the stale checksum during edits; those
identity bytes never enter the encoded save. Invalid sizes, checksums, shifts,
game names and document identities are rejected with localizable failures.

The folder model has 45 native operations per template, comparing every byte
and all folder/pack getter snapshots after each mutation. These include the
active Regular-chip cache, empty slots, pack counts, anticheat data and rejected
indices. BN5 uses the extracted `bn-common/save/folders` implementation too;
its existing model and editor reference tests still pass. BN4's complete fixture
suite runs each template in English and Japanese in fresh callbacks, without
increasing the runtime budget. These save checks do not establish editor or
match parity.

NaviCust, Mod Cards and Auto Battle Data have another 269 native operations per
template (3,228 total). Every mutation compares the whole decoded buffer, all
relevant getters, HP/folder-limit derivation and Mod Card legality. Cases include
all placement slots, rotations, compression fallback, overlapping parts, invalid
grid/color bytes, enabled and disabled fixed card slots, anticheat mirrors,
primary/secondary use counts, ties, and the complete materialized Auto Battle
Data list. Derived-stat cases cover duplicate cells, HP programs away from the
command line, folder programs on it, integer wrapping and folder-limit caps.
Native Mod Card effects apply even in the wrong slot; legality reports that
separately, including disabled bindings. Dependent-package method replacements
are observed by existing stat and legality consumers.

The full Mod Card catalog matches all 133 entries in each region, including
printed names, slots, typed effects and bugs. NaviCust catalogs match all 188
part IDs and ten effect groups. Shared NaviCust and Auto Battle Data storage
also pass BN5's existing native model comparisons.

Local ROM comparisons cover US Red Sun/Blue Moon revision 0 and Japanese
Red Sun/Blue Moon revision 1. All 350 chips, 13 element icons and 188 NaviCust
parts per cartridge match native names, descriptions, fields, bitmaps and RGBA
pixels. No ROM bytes or captured graphics are checked in. Synthetic ROM tests
cover text commands, e-Reader substitutions, chip field ordering, palette
transparency and part color encoding. BN4 exposes no compressed part bitmaps,
matching the native reader: the game does not use them and UnderSht's ROM bitmap
is incorrect. Shared chip and part readers retain BN5/BN6's default layouts.
