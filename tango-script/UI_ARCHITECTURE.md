# A general scripted editor toolkit

The UI host must support editors for arbitrary save formats and games. All
game-specific interpretation lives in Luau, including the composition and
behavior of the editor. The BN editor's compatibility and usability requirements
apply to its package; they do not define the vocabulary of the host API. Prefer
a simpler implementation over exact native animation or pixel matching.

## Boundaries

| Rust host | Luau package |
| --- | --- |
| Layout, text shaping, font/image handles, themes and generic widgets | Sections, labels, domain objects, available controls and layout composition |
| Pointer/keyboard/focus events, scrolling, drag/drop and generic hit regions | Meaning of a gesture, accepted drops, snapping, placement and validation rules |
| Custom drawing through a bounded display list/canvas | Boards, maps, part outlines, diagrams, previews and other specialized visuals |
| Virtualized lists/tables/trees and selection mechanics | Inventory/catalog entries, filtering, sort order, columns, summaries and actions |
| Transaction history and explicit serializable document/view state | Save decoding, encoding, edits, validation and derived values |
| Explicit file/clipboard/image-export effects and embedding capabilities | What to export and when to request an allowed effect |

There must be no host widget or event such as `BNFolder`, `NaviCustPart`,
`SetChipCode`, or `ToggleRegular`. A script can define those concepts using
generic lists, images, drawing commands and typed application actions. The same
primitives should support a creature/team editor, RPG inventory/equipment editor,
world/quest editor, racing garage editor, or a structured binary/JSON editor.

## UI surface to build

Use a declarative tree for normal UI and an imperative display list for custom
drawing. Both travel through one game-neutral renderer. A package should be able
to combine them, such as a scrollable searchable library beside a custom canvas.

- Layout: rows/columns, wrapping, grids, stacks, split panes, clipping, scrolling,
  sizing constraints and responsive composition.
- Widgets: text, images, buttons, checkboxes, sliders, numeric/text inputs,
  choices, tabs, tooltips, menus and dialogs, with disabled/selected/focus states.
- Large collections: virtualized lists/tables/trees, stable item identities,
  multi-selection, filtering, sorting and drag reordering.
- Canvas: transforms, clips, shapes, lines, text and images; explicit hit regions
  and pointer coordinates; hover, press/release, wheel, pointer capture and drag.
  The package computes domain-specific geometry and placement rules.
- Effects: copy text/HTML/image, export a file, request a permitted host action,
  and invalidate a view. Effects are validated separately from model updates.
- Presentation: inherit the embedding Tango theme, fonts, metrics, localization
  context, accessibility/focus semantics and animation primitives. Domain labels
  and tooltips remain package-owned. Visual parity uses the existing neutral
  `tango-ui` controls, extending those controls when necessary.

Input events should carry typed payloads (text, number, selection, coordinates,
modifiers, drag source/target), not game-specific Rust enums or increasingly
complicated strings. Packages translate those events into their own typed domain
actions. Keep widget identity stable across renders to preserve focus, selection,
scroll position and in-progress gestures.

Collection positions are 1-based throughout the scripting API. List `reorder`
events can feed `table.remove(items, action.from)` and
`table.insert(items, action.to, moved)` directly. `tone.row` uses the same row
position; Rust converts it at the native widget boundary. Package model APIs
follow this convention too: folder/slot positions, Regular/Tag selections,
NaviCust/card slots, pack variants and message-table entries start at 1. Byte
offsets, pixel coordinates and encoded game IDs retain their format-defined
values; a game ID is a lookup key, not an array position.

## Data and embedding

Support explicit serializable structured models as the UI API grows. A script
may want a party/inventory tree or parsed JSON as its model instead of editing
raw binary offsets. Buffers remain useful for binary data and images. Lua
functions, VM handles and arbitrary userdata cannot become persistent model
state; those would defeat reproducible transactions, reload and rollback.
Document state and ephemeral UI state must be separate, with an explicit
restoration policy so pending inputs cannot hide an undone edit.

Provide read-only ROM/asset inputs through the package environment. Packages
decode their own graphics, strings, schemas and format metadata. Generic
decompression/checksum/image operations can be host capabilities where needed;
the host must not acquire a game's offsets or file-layout knowledge.

Embedding supplies capabilities and context: locale/theme/scale, viewport,
read-only versus editable mode, and allowed actions such as Play or clipboard
export. A replay/opponent viewer must reject edit actions at the host boundary,
even if a script emits one. Save preparation returns bytes to the embedder;
only a successful write commits the document and changes the edit-session state.
The game-neutral editor component must not open its own window or own app paths.

The same renderer can embed a base package or an extension-modified editor. A
patch that changes a save format, adds items, or introduces a new section can
override the decoder, rules and UI in its extension. No new Rust game adapter is
needed for those changes.

## Current implementation versus destination

The current host is already game-neutral: it consumes buffers, a generic
node tree, and actions. Its host tests use self-contained synthetic documents.
`bn6` supplies its own save interpretation and folder UI in Luau.

The current Serde-derived schema supports layout containers, text, raster images,
inputs, choices, buttons, checkboxes, sliders, reorderable lists, and canvas
display lists with typed pointer/keyboard events. `tango-script-iced` renders these
using Tango's shared styles. Host tests exercise the interactive toolkit without
a game package. Geometry and domain interpretation stay in Luau.
The portable schema is `tango_script::ui`; `tango-script-iced` is only the Iced
rendering backend. Alternating/selected rows and tinted buttons reuse native
Tango styles. Generic appearance overrides, inherited foregrounds/fonts, control
metrics and rich node-based tooltips allow scripts to compose the reference
editor's presentation. Software-rendered pixel/event comparisons cover those
primitives in light/dark themes and active/hover/pressed/disabled states; they do
not establish parity for a complete game editor.
Package-owned Fluent catalogs supply translated labels, tooltips and diagnostics;
`Profile::with_locale` and transactional `Document::set_locale` provide the host
language context. Shared translations use ordinary package dependencies. See the
[localization contract](README.md#localization) for fallback and formatting rules.

`EditorContext` now supplies a host-owned `read_only` capability to view/update
callbacks. Read-only documents can change view state, but document mutations are
rejected before committing any state or rebuilding the view. The development
host exposes this through `--read-only` and disables save actions. The BN6 package
uses it for the identity strip and folder, NaviCust and patch-card viewers.
Navigation and hover observations update view state without changing the save.
`EditorContext.embedding` supplies `inline_actions`, `streamer_mode`, and an
application-defined map of action capabilities. `Embedding::default()` exposes
no host actions and disables streamer mode.
The embedder can use `EditorSession::open_embedded` and transactional
`set_embedding` to grant or revoke them without resetting edits or preferences.
Each action has `enabled` and `while_editing` flags; the callback sees its
effective availability, and the host checks the original capabilities when an
update returns `{kind = "invoke", id = "..."}`. Scripts cannot grant capabilities
by modifying the context table. Requests are also checked after edit transitions,
and no action is available while a save awaits acknowledgment. Rejected updates
release no effects and commit no document/view-state changes. The application
executes permitted effects once after dispatch succeeds.

Names such as `play` belong to the app and package contract; the script host has
no Play or game action enum. BN6 composes its Play button from ordinary nodes and
omits it while editing. `inline_actions = false` moves its edit and contextual
export controls out of the scripted view; the embedder can still call
`EditorSession::edit`. This control placement flag does not grant capabilities.
The app adapter and remaining embedding presentation are still being ported.
`EditorSession::configure` applies locale, edit permission and embedding options
together. Unchanged options skip script execution. Revoking edit permission
retains the draft and its history while rejecting mutations, saves and undo/redo;
the host can restore permission or cancel. Failed refreshes preserve the previous
configuration, so the adapter must not dispatch under stale permissions after a
failed revocation.

Streamer cover/reveal behavior belongs to the package. BN6 owns its regional
PNG logos, decodes them through bounded `tango.decode_png`, and composes the
cover through `editor-common/cover` using ordinary nodes. Review changes view
state even in read-only panels, survives edit/locale/preference changes, and
resets when a new document is opened. The Rust host has no game logo registry
or cover widget in this path.

Nodes can attach bounded entrance motion with a key, revision, pixel offset,
duration, delay and easing. The renderer retains clocks across view rebuilds and
evicts removed descriptors. It requests presentation frames directly, without
running Luau or mutating document state. `None` messages only refresh the host
view. Drawing and pointer handling use the same native floating transform;
layout retains the subtree's resting dimensions. `view_at` permits deterministic
frame sampling for parity checks.
Rendered elements own their data and share bounded image resources. They do not
borrow the document or renderer, so a panel can release its session guard before
Iced performs layout, input handling or drawing. The renderer itself is Send/Sync;
`restart_motion` restarts its clocks without invoking the package.
BN6 owns the restart policy in `editor/motion.luau`: vertical on save arrival,
Review and edit transitions; horizontal in strip order on tab changes. Shared
tail controls remain anchored and only newly appearing controls slide in. The
picker opens, dismisses and selects using the same 160 ms vertical entrance for
the entire region below the header. Rapid toggles restart this entrance; no
hidden branch or reversible clock is retained. The body resumes its own entrance
policy on the next tab or session change. Edit/Save/Cancel controls switch
immediately instead of retaining outgoing controls for a two-phase morph.
Navi roster groups and header statistics use ordinary wrapping layouts so
their content remains accessible in narrow panels.

`EditorSession` now owns the whole-document Edit/Save/Cancel lifecycle. It keeps
the baseline and history outside package state, rejects writes outside edit mode,
and prepares both success and failure views before handing storage an opaque
save receipt. A failed write preserves staged edits; a successful acknowledgment
adopts the model represented by those bytes. Package-owned `reset_view` keeps view
preferences across transitions. `editor-common/session` composes the controls from
ordinary nodes; BN6 places them in its identity strip. The development window now
uses this same session API. The normal Play, replay and in-match setup panels now
attach a bundled or installed editor when exactly one editor recognizes the effective ROM
through its optional `detect_rom` callback. BN6 supplies that recognition in Luau.
The native embedding boundary now separates `SaveEditorFactory` (native model
preparation) from `SaveEditor` (render/update/snapshot). The Play host performs
atomic writes and calls `save_finished` on either success or failure; the package
adapter uses that acknowledgment to complete the session's opaque receipt.
Messages carry a per-save identity and configuration generation. Failed panel
configuration hides the stale view and blocks its actions, including queued ones.
The app retains all returned UI tasks and host requests. Clipboard drawings use
the active app theme, and snapshot failures propagate to the launch/commit caller.
Legacy patched ROMs retain the native editor until their ROM overrides migrate
to packages. Native game selection, launch and chart metadata are still used;
this editor attachment does not replace the simulation or telemetry drivers.

Canvases can rasterize their display lists at a declared source resolution,
then scale the cached image, or draw directly in fixed display coordinates.
The renderer shares the generic shape/text/image implementation between both
paths. Generated images and rasterization work have explicit budgets; their
cache identity includes commands, font and theme. The NaviCust package uses
this for its stored-grid image and a separate fixed-coordinate hover outline.
All part geometry, colors, hit testing and warning rules remain in Luau.

Updates can return validated text, HTML and image clipboard effects. The document
releases effects only after its transaction commits; observers and undo/redo do
not replay them. Read-only documents can copy without gaining mutation access.
The desktop host handles clipboard transport and HTML fallback, then acknowledges
successful requests to show package-owned transient button content. Copy glyphs,
tooltips, export formats, and the decision to offer a copy remain in Luau.
Generic native PNG/Base64 encoding and drawing rasterization keep image export
bounded without implementing game-specific exporters in Rust.

The buffer-only document model and standalone development window do **not** yet
implement the complete toolkit above. Stable keyed widget reconciliation,
virtualized collections, additional file/embedding effects, accessibility for canvas content, and
complete focus/navigation behavior remain to be implemented. These primitives
cannot yet reproduce the existing rich BN editor.
Expand and test those generic capabilities while porting the reference editor;
do not freeze the development SDK or publish a compatibility promise prematurely.

[EDITOR_PARITY.md](EDITOR_PARITY.md) defines the acceptance cases for replacing
Tango's current embedded editors. Each extracted host primitive must also be
usable without the BN package or its native support crates.
