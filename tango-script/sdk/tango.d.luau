-- Tango's package SDK: capability exports, UI, files, inputs, translations and host utilities.
-- These definitions are supplied to both the package checker and Luau LSP.
-- Naming: snake_case values/fields, PascalCase types, SCREAMING_SNAKE_CASE constants.
-- Imports use the standard global require; tango contains host capabilities.
export type Modifiers = { shift: boolean, control: boolean, alt: boolean, command: boolean }
export type PointerPhase = "move" | "down" | "up" | "leave" | "wheel"
export type PointerButton = "none" | "left" | "middle" | "right" | "other"
export type PointerFilter = {phase: PointerPhase, button: PointerButton?}
-- Collection positions (including reorder from/to and tone.row) are 1-based.
-- Pixel coordinates and buffer byte offsets retain their normal zero origin.
export type Action =
    { id: string, kind: "change", value: string }
    | { id: string, kind: "activate", value: string }
    | { id: string, kind: "toggle", checked: boolean }
    | { id: string, kind: "slide", number: number }
    | { id: string, kind: "scroll", x: number, y: number }
    | { id: string, kind: "pointer", phase: PointerPhase,
        x: number, y: number, button: PointerButton,
        dx: number, dy: number, modifiers: Modifiers }
    | { id: string, kind: "key", key: string, pressed: boolean, modifiers: Modifiers }
    | { id: string, kind: "reorder", from: number, to: number }

-- Absent/deleted fields are nil; values that survive an action are strings.
export type ViewState = { [string]: string? }
export type Choice = { value: string, label: string }
export type Size = number | "fill" | "shrink" | { portion: number }
export type Align = "start" | "center" | "end"
export type Tone = "neutral" | "primary" | "danger" | "muted" | "selected" | "plate" | "card" | "transparent" | "tooltip"
    | {row: number, selected: boolean?} | {tint: Color} | {tab: boolean}
-- RGBA and padding arrays have exactly four components.
export type ThemeColor = "text" | "muted" | "primary" | "danger" | "danger_strong" | "success" | "warning"
    | "background" | "plate" | "border" | "transparent"
export type Color = ThemeColor | {number} | {theme: ThemeColor, alpha: number}
    | {theme: ThemeColor, tint: {number}, mix: number}
export type Shadow = {color: Color, offset: {number}?, blur: number?}
-- Linear gradients use radians and 1–8 stops with increasing offsets in 0..1.
export type Background = Color | {angle: number, stops: {{offset: number, color: Color}}}
export type Appearance = {
    background: Background?, text_color: Color?, border_color: Color?, border_width: number?, radius: number?,
    shadow: Shadow?,
}
-- Content is presentation-only and shares the enclosing view's resource budget.
export type TooltipPosition = "top" | "bottom" | "left" | "right" | "follow_cursor"
export type Tooltip = string | {
    content: Node, position: TooltipPosition?, gap: number?,
}
export type ButtonFeedback = {child: Node, tooltip: Tooltip?}
-- Cursor hints change the affordance without capturing input.
export type Cursor = "default" | "hidden" | "context_menu" | "help" | "pointer" | "progress" | "wait"
    | "cell" | "crosshair" | "text" | "alias" | "copy" | "move" | "no_drop" | "not_allowed" | "grab" | "grabbing"
    | "resize_horizontal" | "resize_vertical" | "resize_diagonal_up" | "resize_diagonal_down"
    | "resize_column" | "resize_row" | "all_scroll" | "zoom_in" | "zoom_out"
export type Font = "normal" | "mono" | "icons"
export type Wrapping = "none" | "word" | "glyph" | "word_or_glyph"
export type TextSize = "title" | "heading" | "body" | "caption" | number
export type Raster = { width: number, height: number, rgba: buffer }
export type Drawing = {width: number, height: number, commands: {Draw}, corner_radius: number?}
export type ImageSource = Raster | Drawing
export type Effect = {kind: "copy_text", text: string}
    | {kind: "invoke", id: string}
    | {kind: "copy_html", text: string, html: string}
    | {kind: "copy_image", image: ImageSource}
    | {kind: "edit", command: EditCommand}
    -- Relative offsets; IDs are confined to this document's renderer.
    | {kind: "scroll_to", id: string, x: number, y: number}
export type EditCommand = "begin" | "cancel" | "save"
export type LineCap = "butt" | "square" | "round"
export type Paint = { fill: Color?, stroke: Color?, stroke_width: number?, line_cap: LineCap? }
export type Draw =
    { kind: "rect", x: number?, y: number?, width: number, height: number, radius: number?, fill: Color?, stroke: Color?, stroke_width: number?, line_cap: LineCap? }
    | { kind: "ellipse", x: number?, y: number?, rx: number, ry: number, fill: Color?, stroke: Color?, stroke_width: number?, line_cap: LineCap? }
    | { kind: "path", points: {{number}}, closed: boolean?, fill: Color?, stroke: Color?, stroke_width: number?, line_cap: LineCap? }
    | { kind: "text", x: number?, y: number?, text: string, size: TextSize?, font: Font?, color: Color? }
    | { kind: "image", x: number?, y: number?, width: number, height: number, image: Raster, nearest: boolean? }

-- Motion is presentation-only: layout stays at rest; drawing and pointer handling move.
-- A new key or changed descriptor starts an entrance; unchanged revisions retain time.
export type Easing = "linear" | "ease_in_cubic" | "ease_out_cubic" | "ease_in_out_cubic"
export type Motion = {key: string, revision: string, from: {number},
    duration_ms: number?, delay_ms: number?, easing: Easing?}

export type Layout = {
    key: string?, motion: Motion?, enabled: boolean?, tooltip: Tooltip?, cursor: Cursor?, tone: Tone?, appearance: Appearance?,
    width: Size?, height: Size?, padding: (number | {number})?, spacing: number?,
    align_x: Align?, align_y: Align?, max_width: number?, max_height: number?, clip: boolean?,
}
export type Node =
    { kind: "column", children: {Node},
        key: string?, motion: Motion?, enabled: boolean?, tooltip: Tooltip?, cursor: Cursor?, tone: Tone?, appearance: Appearance?,
        width: Size?, height: Size?, padding: (number | {number})?, spacing: number?,
        align_x: Align?, align_y: Align?, max_width: number?, max_height: number?, clip: boolean?,
    }
    | { kind: "row", children: {Node},
        key: string?, motion: Motion?, enabled: boolean?, tooltip: Tooltip?, cursor: Cursor?, tone: Tone?, appearance: Appearance?,
        width: Size?, height: Size?, padding: (number | {number})?, spacing: number?,
        align_x: Align?, align_y: Align?, max_width: number?, max_height: number?, clip: boolean?,
    }
    | { kind: "wrap", children: {Node},
        key: string?, motion: Motion?, enabled: boolean?, tooltip: Tooltip?, cursor: Cursor?, tone: Tone?, appearance: Appearance?,
        width: Size?, height: Size?, padding: (number | {number})?, spacing: number?,
        align_x: Align?, align_y: Align?, max_width: number?, max_height: number?, clip: boolean?,
    }
    | { kind: "stack", children: {Node},
        key: string?, motion: Motion?, enabled: boolean?, tooltip: Tooltip?, cursor: Cursor?, tone: Tone?, appearance: Appearance?,
        width: Size?, height: Size?, padding: (number | {number})?, spacing: number?,
        align_x: Align?, align_y: Align?, max_width: number?, max_height: number?, clip: boolean?,
    }
    | { kind: "container", child: Node,
        key: string?, motion: Motion?, enabled: boolean?, tooltip: Tooltip?, cursor: Cursor?, tone: Tone?, appearance: Appearance?,
        width: Size?, height: Size?, padding: (number | {number})?, spacing: number?,
        align_x: Align?, align_y: Align?, max_width: number?, max_height: number?, clip: boolean?,
    }
    | { kind: "scroll", child: Node, horizontal: boolean?, id: string?, on_scroll: boolean?, scrollbar: ("auto" | "hover" | "hidden")?,
        key: string?, motion: Motion?, enabled: boolean?, tooltip: Tooltip?, cursor: Cursor?, tone: Tone?, appearance: Appearance?,
        width: Size?, height: Size?, padding: (number | {number})?, spacing: number?,
        align_x: Align?, align_y: Align?, max_width: number?, max_height: number?, clip: boolean?,
    }
    | { kind: "list", id: string?, children: {Node}, reorder: boolean?,
        key: string?, motion: Motion?, enabled: boolean?, tooltip: Tooltip?, cursor: Cursor?, tone: Tone?, appearance: Appearance?,
        width: Size?, height: Size?, padding: (number | {number})?, spacing: number?,
        align_x: Align?, align_y: Align?, max_width: number?, max_height: number?, clip: boolean?,
    }
    | { kind: "text", text: string, size: TextSize?, font: Font?, color: Color?, strikethrough: boolean?, underline: boolean?, wrapping: Wrapping?,
        key: string?, motion: Motion?, enabled: boolean?, tooltip: Tooltip?, cursor: Cursor?, tone: Tone?, appearance: Appearance?,
        width: Size?, height: Size?, padding: (number | {number})?, spacing: number?,
        align_x: Align?, align_y: Align?, max_width: number?, max_height: number?, clip: boolean?,
    }
    | { kind: "image", image: Raster, nearest: boolean?, opacity: number?,
        key: string?, motion: Motion?, enabled: boolean?, tooltip: Tooltip?, cursor: Cursor?, tone: Tone?, appearance: Appearance?,
        width: Size?, height: Size?, padding: (number | {number})?, spacing: number?,
        align_x: Align?, align_y: Align?, max_width: number?, max_height: number?, clip: boolean?,
    }
    | { kind: "input", id: string, text: string?, value: string?, placeholder: string?, secure: boolean?, content_padding: (number | {number})?, size: TextSize?,
        key: string?, motion: Motion?, enabled: boolean?, tooltip: Tooltip?, cursor: Cursor?, tone: Tone?, appearance: Appearance?,
        width: Size?, height: Size?, padding: (number | {number})?, spacing: number?,
        align_x: Align?, align_y: Align?, max_width: number?, max_height: number?, clip: boolean?,
    }
    | { kind: "select", id: string, text: string?, value: string, choices: {Choice}, content_padding: (number | {number})?, size: TextSize?,
        key: string?, motion: Motion?, enabled: boolean?, tooltip: Tooltip?, cursor: Cursor?, tone: Tone?, appearance: Appearance?,
        width: Size?, height: Size?, padding: (number | {number})?, spacing: number?,
        align_x: Align?, align_y: Align?, max_width: number?, max_height: number?, clip: boolean?,
    }
    | { kind: "button", id: string, action_enabled: boolean?, value: string?, text: string?, child: Node?, size: TextSize?, font: Font?,
        feedback: ButtonFeedback?, content_padding: (number | {number})?, hovered: Appearance?, pressed: Appearance?, disabled: Appearance?,
        key: string?, motion: Motion?, enabled: boolean?, tooltip: Tooltip?, cursor: Cursor?, tone: Tone?, appearance: Appearance?,
        width: Size?, height: Size?, padding: (number | {number})?, spacing: number?,
        align_x: Align?, align_y: Align?, max_width: number?, max_height: number?, clip: boolean?,
    }
    | { kind: "checkbox", id: string, text: string, checked: boolean, size: number?, text_size: TextSize?,
        key: string?, motion: Motion?, enabled: boolean?, tooltip: Tooltip?, cursor: Cursor?, tone: Tone?, appearance: Appearance?,
        width: Size?, height: Size?, padding: (number | {number})?, spacing: number?,
        align_x: Align?, align_y: Align?, max_width: number?, max_height: number?, clip: boolean?,
    }
    | { kind: "slider", id: string, number: number, min: number?, max: number?, step: number?,
        key: string?, motion: Motion?, enabled: boolean?, tooltip: Tooltip?, cursor: Cursor?, tone: Tone?, appearance: Appearance?,
        width: Size?, height: Size?, padding: (number | {number})?, spacing: number?,
        align_x: Align?, align_y: Align?, max_width: number?, max_height: number?, clip: boolean?,
    }
    | { kind: "canvas", id: string?, canvas_width: number, canvas_height: number,
        pointer: boolean?, pointer_capture: (boolean | {PointerFilter})?, keyboard: boolean?, commands: {Draw},
        -- Bake at integer canvas dimensions, then fit like an image (linear by default).
        rasterize: {corner_radius: number?, nearest: boolean?}?,
        -- Defaults to true; false keeps drawing and pointer units in display pixels.
        scale_to_fit: boolean?,
        key: string?, motion: Motion?, enabled: boolean?, tooltip: Tooltip?, cursor: Cursor?, tone: Tone?, appearance: Appearance?,
        width: Size?, height: Size?, padding: (number | {number})?, spacing: number?,
        align_x: Align?, align_y: Align?, max_width: number?, max_height: number?, clip: boolean?,
    }
    | { kind: "space",
        key: string?, motion: Motion?, enabled: boolean?, tooltip: Tooltip?, cursor: Cursor?, tone: Tone?, appearance: Appearance?,
        width: Size?, height: Size?, padding: (number | {number})?, spacing: number?,
        align_x: Align?, align_y: Align?, max_width: number?, max_height: number?, clip: boolean?,
    }
-- Host capabilities cannot be changed through view state or callback arguments.
export type EditContext = {editable: boolean, editing: boolean, saving: boolean, can_save: boolean}
export type HostAction = {enabled: boolean, while_editing: boolean}
export type Embedding = {inline_actions: boolean, streamer_mode: boolean, actions: {[string]: HostAction}}
export type EditorContext = {read_only: boolean, session: EditContext?, embedding: Embedding}
export type SaveTemplate = {name: string, label: string}
-- Script-owned capability tables are mutable: dependent packages can clone or
-- wrap their methods. The host checks the readable callback contract.
export type Editor = {
    -- Optional, paired callbacks for the host's New Save flow.
    templates: (() -> {SaveTemplate})?,
    create_save: ((string) -> buffer)?,
    -- Recognize an effective ROM for automatic selection. Omit for manual use.
    detect_rom: ((buffer) -> boolean)?,
    decode: (buffer) -> buffer,
    encode: (buffer) -> buffer,
    validate: (buffer) -> {string},
    view: (buffer, ViewState, EditorContext) -> Node,
    -- Return host effects only from an update; the host releases them after commit.
    update: (buffer, ViewState, Action, EditorContext) -> {Effect}?,
    -- Retain view preferences and discard scratch state on begin, cancel and save.
    -- Omitting this callback clears all view state at an edit transition.
    reset_view: ((ViewState) -> ())?,
    test: (() -> ())?,
}
-- Each capability is a separate module export. Import dependencies explicitly;
-- package tests live in init.luau and do not require any capability selection.
export type GameModeContext = {
    player: number, seed: {number}, disable_bgm: boolean,
    options: {[string]: boolean | number | string},
}
-- Register hooks during setup; read/write the emulator inside their callbacks.
-- Reads observe earlier writes in the same callback. A failed callback commits
-- nothing. Setup constants may be captured; mutable locals do not persist
-- between callbacks. Changing game state belongs in emulator memory/registers.
export type Game = Memory & {
    read hook: (number, () -> ()) -> (),
    read write: (string, number, buffer) -> (),
    read read_register: (string) -> number,
    read write_register: (string, number) -> (),
    read ready: () -> (),
    read round_started: () -> (),
    read round_outcome: (number?) -> (),
    read match_ended: () -> (),
    read match_aborted: () -> (),
}
-- Old format fields remain confined to import; variant and match codes retain
-- their encoded values, unlike collection positions and player numbers.
export type LegacyReplay = {
    game: {rom_family: string, rom_variant: number, sim_version: number, patch: {name: string, version: string}?},
    match_type: number, match_subtype: number,
    engine: {name: string, revision: number},
}
export type ReplayImport = {disable_bgm: boolean?, options: {[string]: boolean | number | string}?}
export type GameMode = {
    -- Emulator platform for the normal match selector (currently "gba").
    platform: string?,
    detect_rom: ((buffer) -> boolean)?,
    patch_rom: ((buffer) -> buffer)?,
    -- Return nil unless this exact legacy header and ROM can be simulated.
    import_replay: ((LegacyReplay, buffer) -> ReplayImport?)?,
    -- Omit setup for a ROM-preparation-only export.
    setup: ((GameModeContext, Game) -> ())?,
}
export type TelemetryValue = boolean | number | string
export type TelemetryEvent = {name: string, fields: {[string]: TelemetryValue}}
export type TelemetryFrame = {values: {[string]: TelemetryValue}, events: {TelemetryEvent}}
-- Player positions are 1-based; tick is the host simulation tick.
export type TelemetryContext = {tick: number, round: number, player: number}
-- Host-defined address spaces, byte addresses and lengths (not collection indices).
-- Reads return independent buffers and cannot change emulator memory.
export type Memory = {read read: (string, number, number) -> buffer}
export type TelemetryRange = {player: number, name: string, from_tick: number, to_tick: number}
-- Numeric envelopes retain extrema inside each inclusive bucket. Missing or
-- nonnumeric values create gaps; indices are 1-based and ticks keep their timebase.
export type TelemetryBucket = {from_tick: number, to_tick: number, first: number?, last: number?, min: number?, max: number?, gap: boolean}
export type TelemetryHistory = {
    read value: ({player: number, name: string, tick: number}) -> TelemetryValue?,
    read series: (TelemetryRange, number) -> {TelemetryBucket},
    read count_events: (TelemetryRange) -> number,
}
export type TelemetryViewContext = {player: number, from_tick: number, to_tick: number, ticks_per_second: number, incomplete: boolean}
export type Telemetry = {
    detect_rom: ((buffer) -> boolean)?,
    -- Omit for an empty initial state (for example, a stateless poller).
    initial_state: (() -> buffer)?,
    -- Read the current core through memory, and immutable inputs through
    -- tango.read_input. Only the private state buffer is mutable across ticks;
    -- the host snapshots it with the emulator for rollback.
    poll: (buffer, TelemetryContext, Memory) -> TelemetryFrame,
    view: ((TelemetryHistory, TelemetryViewContext, ViewState) -> Node)?,
    update_view: ((ViewState, Action) -> ViewState)?,
}
export type TranslationArgs = { [string]: string | number }
export type Translate = (string, TranslationArgs?) -> string
declare tango: {
    -- Decode the first PNG frame into RGBA8 (max 4096 per axis, 32 MiB decoded).
    -- Encoded input and decompression share the operation's host work budget.
    decode_png: (buffer) -> Raster,
    -- Package-local module-relative path; import shared translators via require.
    -- Missing keys/attributes delegate to the optional imported translator.
    load_catalog: (string, Translate?) -> Translate,
    -- Raise a retained user-facing error. Uses a load_catalog translator and
    -- catalog fallbacks; no script is rerun when the UI language changes.
    fail: (Translate, string, TranslationArgs?) -> never,
    locale: () -> string,
    text_direction: () -> ("ltr" | "rtl" | "ttb"),
    -- Unicode lowercase, independent of the active locale (16 KiB text limit).
    lowercase: (string) -> string,
    -- Bounded native encoders. PNG preserves alpha; Base64 is standard and padded.
    encode_png: (Raster) -> buffer,
    base64_encode: (buffer) -> string,
    read_file: (string) -> buffer,
    input_size: (string) -> number?,
    read_input: (string, number, number) -> buffer,
    -- BIOS LZ77 (0x10), optional zero-based offset (default 0). Malformed data
    -- returns nil; invalid arguments and exhausted host budgets raise errors.
    crc32: (buffer) -> number,
    unlz77: (buffer, number?) -> buffer?,
    bps: (buffer, buffer) -> buffer,
}
