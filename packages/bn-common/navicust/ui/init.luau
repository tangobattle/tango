--!strict
local types = require("../types")
local shape = require("../shape")
local catalog = require("../catalog")
local edit = require("../edit")
local controller = require("../state")
local grid = require("./grid")
local thumbnail = require("./thumbnail")
local ui = require("@editor-common/ui")
local text = require("@editor-common/text")

local function icon_button(id: string, icon: string, label: string, enabled: boolean): Node
    return {kind = "button", id = id, text = icon, font = "icons" :: Font, enabled = enabled,
        content_padding = {6, 8, 6, 8}, tooltip = {position = "bottom" :: TooltipPosition, gap = 4, content = {
            kind = "container" :: "container", padding = 6,
            appearance = {background = "border" :: Color, text_color = "text" :: Color, radius = 4, border_width = 1,
                border_color = {theme = "primary", alpha = 0.45} :: Color},
            child = {kind = "text" :: "text", text = label, size = "caption" :: TextSize},
        }}}
end

local function view(parts: types.View, assets: types.Assets, state: ViewState, illegal: {[number]: boolean}, t: Translate): Node
    local current = controller.read(state)
    local occupancy = shape.materialize(parts, assets)
    local g = grid.geometry(assets.layout)
    local occupied = if current.hover then occupancy[current.hover.row * assets.layout.cols + current.hover.col + 1] else 0
    local cursor: Cursor? = if current.held then "grabbing" else if occupied and occupied > 0 then "grab" else nil
    local capture: {PointerFilter} = {}
    if current.held then
        table.insert(capture, {phase = "down" :: PointerPhase, button = "right" :: PointerButton})
        table.insert(capture, {phase = "wheel" :: PointerPhase})
        local ghost = if current.hover then shape.ghost(current.held, current.hover, assets, occupancy) else nil
        if ghost and ghost.legal then table.insert(capture, {phase = "down" :: PointerPhase, button = "left" :: PointerButton}) end
    elseif occupied and occupied > 0 then table.insert(capture, {phase = "down" :: PointerPhase, button = "left" :: PointerButton}) end
    local canvas: Node = {kind = "canvas", id = "navicust.board", pointer = true,
        pointer_capture = capture,
        -- The native board keeps its display scale when a narrow pane clips it.
        scale_to_fit = false,
        canvas_width = g.width, canvas_height = g.height, width = g.width, height = g.height, cursor = cursor,
        commands = grid.commands(parts, assets, occupancy, current.held, current.hover)}
    local board: Node = {kind = "stack", width = g.width, height = g.height,
        children = {canvas, require("./tooltips").view(parts, assets, occupancy)}}
    local installed = 0
    for slot = 1, parts.count() do if parts.part(slot) then installed += 1 end end
    local inner: {Node} = {{kind = "container", width = "fill" :: Size, align_x = "center" :: Align, child = board}}
    local badges = require("./installed").view(parts, assets, illegal, t)
    if badges then table.insert(inner, badges) end
    local header: Node = {kind = "container", padding = {8, 12, 8, 12}, width = "fill" :: Size, child = ui.row({
        {kind = "text" :: "text", text = t("navicust-edit-grid")}, ui.caption(t("navicust-edit-count", {count = installed}), false),
        {kind = "space" :: "space", width = "fill" :: Size}, ui.clear_button("navicust.clear", t("save-edit-clear")),
    }, 8)}
    local body: Node = {kind = "column", spacing = 8, align_x = "center" :: Align, padding = {4, 8, 4, 8}, width = "fill" :: Size, children = inner}
    local counts = edit.counts(parts)
    local palette: {Node} = {}
    for index, entry in catalog.entries(assets, current.sort, current.filter) do
        local info = assets.part(entry.id) :: types.Info
        local at_cap = (counts[entry.id] or 0) >= edit.MAX_COPIES
        local selected = current.held ~= nil and current.held.id == entry.id
        local rot, compressed = controller.orientation(state, entry.id)
        local labels: {Node} = {{kind = "text", text = entry.name, color = if at_cap then "muted" :: Color else nil}}
        if entry.description and not text.is_blank(entry.description) then
            table.insert(labels, {kind = "text", text = entry.description, size = "caption" :: TextSize,
                color = if selected then nil else "muted" :: Color} :: Node)
        end
        local controls: Node = {kind = "column", spacing = 4, align_x = "center" :: Align, children = {
            icon_button("navicust.rotate:" .. tostring(entry.id), "\u{e149}", t("navicust-edit-rotate"), true),
            icon_button("navicust.compress:" .. tostring(entry.id), if compressed then "\u{e21a}" else "\u{e220}",
                t(if compressed then "navicust-edit-uncompress" else "navicust-edit-compress"), shape.compressible(info)),
        }}
        local content: Node = {kind = "row", width = "fill" :: Size, spacing = 8, align_y = "center" :: Align, children = {
            thumbnail.view(info, rot, compressed, at_cap), {kind = "column" :: "column", spacing = 1, children = labels},
            {kind = "space" :: "space", width = "fill" :: Size}, controls,
        }}
        -- Disable only the outer pick action; nested orientation controls stay usable.
        table.insert(palette, {kind = "button", id = "navicust.pick:" .. tostring(entry.id), action_enabled = not at_cap,
            width = "fill" :: Size, tone = {row = index, selected = selected} :: Tone, content_padding = {6, 10, 6, 10}, child = content} :: Node)
    end
    local choices: {Choice} = {}
    for _, sort in catalog.sorts do table.insert(choices, {value = sort, label = t("navicust-sort-" .. sort)}) end
    local palette_header = ui.library_header(t("navicust-edit-search"), current.filter, choices, current.sort,
        t("save-edit-sort"), "navicust.filter", "navicust.sort")
    local panes = ui.row({ui.pane(header, body), ui.pane(palette_header, ui.column(palette, 2))}, 8)
    assert(panes.kind == "row")
    panes.height = "fill" :: Size
    return panes
end

-- These interactions change presentation only. They need the board dimensions,
-- not a save model, decoded ROM assets, occupancy, or a part catalog.
local function update_state(layout: grid.Size, state: ViewState, action: Action): boolean
    if action.kind == "pointer" and action.id == "navicust.board" and (action.phase == "move" or action.phase == "leave") then
        local at = if action.phase == "leave" then nil else grid.cell_at(layout, action.x, action.y)
        state["navicust.hover.col"] = if at then tostring(at.col) else nil
        state["navicust.hover.row"] = if at then tostring(at.row) else nil
    elseif action.kind == "change" and action.id == "navicust.filter" then state["navicust.filter"] = action.value
    elseif action.kind == "change" and action.id == "navicust.sort" and (action.value == "id" or action.value == "name" or action.value == "color") then
        state["navicust.sort"] = action.value
    else return false end
    return true
end

local function update(state: ViewState, parts: types.View, assets: types.Assets, action: Action)
    if update_state(assets.layout, state, action) then return end
    if action.kind == "pointer" and action.id == "navicust.board" then
        local at = if action.phase == "leave" then nil else grid.cell_at(assets.layout, action.x, action.y)
        controller.apply(state, parts, assets, {kind = "hover", cell = at})
        local g = grid.geometry(assets.layout)
        local inside = action.x >= 0 and action.y >= 0 and action.x < g.width and action.y < g.height
        if action.phase == "down" and action.button == "left" and at then
            controller.apply(state, parts, assets, {kind = "cell", col = at.col, row = at.row})
        elseif action.phase == "down" and action.button == "right" and inside then controller.apply(state, parts, assets, {kind = "clear_held"})
        elseif action.phase == "wheel" and inside then controller.apply(state, parts, assets, {kind = "rotate_held"}) end
    elseif action.kind == "activate" then
        if action.id == "navicust.clear" then controller.apply(state, parts, assets, {kind = "clear"}); return end
        local kind, id = string.match(action.id, "^navicust%.(%a+):(%d+)$")
        local number = tonumber(id or "")
        if not number then return end
        if kind == "pick" then controller.apply(state, parts, assets, {kind = "pick", id = number})
        elseif kind == "rotate" then controller.apply(state, parts, assets, {kind = "rotate", id = number})
        elseif kind == "compress" then controller.apply(state, parts, assets, {kind = "compress", id = number}) end
    end
end

return {view = view, update = update, update_state = update_state}
