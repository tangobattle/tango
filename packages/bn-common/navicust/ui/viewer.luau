--!strict
local types = require("../types")
local grid = require("./grid")
local installed = require("./installed")
local tooltips = require("./tooltips")

local scratch = buffer.create(4)
local function f32(value: number): number
    buffer.writef32(scratch, 0, value)
    return buffer.readf32(scratch, 0)
end

local function outline(layout: types.Layout, occupancy: {number}, hover: types.Cell?): {Draw}
    local g = grid.geometry(layout)
    local cell, origin_x, origin_y = f32(g.cell), f32(g.origin_x), f32(g.origin_y)
    local function at(col: number, row: number): number
        return if col >= 0 and row >= 0 and col < layout.cols and row < layout.rows then occupancy[row * layout.cols + col + 1] else 0
    end
    local slot = if hover then at(hover.col, hover.row) else 0
    local result: {Draw} = {}
    if slot == 0 then return result end
    local function line(x1: number, y1: number, x2: number, y2: number)
        table.insert(result, {kind = "path", points = {{x1, y1}, {x2, y2}}, stroke = {1, 1, 1, 1} :: Color,
            stroke_width = math.max(f32(cell * f32(0.12)), 2), line_cap = "square" :: LineCap} :: Draw)
    end
    for row = 0, layout.rows - 1 do
        for col = 0, layout.cols - 1 do
            if at(col, row) ~= slot then continue end
            local x, y = f32(origin_x + f32(col * cell)), f32(origin_y + f32(row * cell))
            local right, bottom = f32(x + cell), f32(y + cell)
            if at(col, row - 1) ~= slot then line(x, y, right, y) end
            if at(col, row + 1) ~= slot then line(x, bottom, right, bottom) end
            if at(col - 1, row) ~= slot then line(x, y, x, bottom) end
            if at(col + 1, row) ~= slot then line(right, y, right, bottom) end
        end
    end
    return result
end

local function view(parts: types.View, assets: types.Assets, occupancy: {number}, illegal: {[number]: boolean}, state: ViewState, t: Translate): Node
    local layout = assets.layout
    local g = grid.geometry(layout)
    local width, height = math.round(g.width), math.round(g.height)
    local col, row = tonumber(state["navicust.viewer.col"] or ""), tonumber(state["navicust.viewer.row"] or "")
    local hover: types.Cell? = if col and row then {col = col, row = row} else nil
    local drawing = grid.drawing(parts, assets, occupancy)
    local image: Node = {kind = "canvas", width = width, height = height,
        canvas_width = drawing.width, canvas_height = drawing.height, commands = drawing.commands,
        rasterize = {corner_radius = math.round(4 / grid.scale)}}
    local board: Node = {kind = "stack", width = width, height = height, children = {
        image, tooltips.view(parts, assets, occupancy),
        {kind = "canvas" :: "canvas", id = "navicust.viewer", pointer = true, pointer_capture = false,
            scale_to_fit = false,
            canvas_width = width, canvas_height = height, width = width, height = height,
            commands = outline(layout, occupancy, hover)} :: Node,
    }}
    local children: {Node} = {board}
    local badges = installed.view(parts, assets, illegal, t)
    if badges then table.insert(children, badges) end
    return {kind = "container", padding = 12, tone = "card" :: Tone, child = {
        kind = "column" :: "column", children = children, spacing = 8, align_x = "center" :: Align,
    }}
end

local function update(layout: grid.Size, state: ViewState, action: Action): boolean
    if action.kind ~= "pointer" or action.id ~= "navicust.viewer" then return false end
    local cell = if action.phase == "leave" then nil else grid.cell_at(layout, action.x, action.y)
    state["navicust.viewer.col"] = if cell then tostring(cell.col) else nil
    state["navicust.viewer.row"] = if cell then tostring(cell.row) else nil
    return true
end

return {view = view, update = update}
