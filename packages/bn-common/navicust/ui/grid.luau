--!strict
local types = require("../types")
local shape = require("../shape")
local colors = require("./colors")
-- Iced uses f32 geometry. Match the reference's scale before multiplying
-- coordinates so corner rasterization is stable at fractional pixel sizes.
local scratch = buffer.create(4)
local function f32(value: number): number
    buffer.writef32(scratch, 0, value)
    return buffer.readf32(scratch, 0)
end
local SCALE = f32(360 / 932)
local RADIUS = f32(f32(4 / SCALE) * SCALE)
local SQUARE, BORDER = 120, 12
local BLACK: Color = {0, 0, 0, 1}
local BACKGROUND: Color = {32 / 255, 32 / 255, 32 / 255, 1}

export type Geometry = {width: number, height: number, origin_x: number, origin_y: number, cell: number}
export type Size = {cols: number, rows: number}
local function geometry(layout: Size): Geometry
    return {width = (layout.cols * SQUARE + 92) * SCALE, height = (layout.rows * SQUARE + 204) * SCALE,
        origin_x = 46 * SCALE, origin_y = 158 * SCALE, cell = SQUARE * SCALE}
end
local function cell_at(layout: Size, x: number, y: number): types.Cell?
    local g = geometry(layout)
    local col, row = math.floor((x - g.origin_x) / g.cell), math.floor((y - g.origin_y) / g.cell)
    return if col >= 0 and row >= 0 and col < layout.cols and row < layout.rows then {col = col, row = row} else nil
end

local function commands(view: types.View, assets: types.Assets, occupancy: {number}, held: types.Held?, hover: types.Cell?, raster: boolean?): {Draw}
    local layout = assets.layout
    local scale = if raster then 1 else SCALE
    local result: {Draw} = {}
    local function rect(x: number, y: number, width: number, height: number, fill: Color?, stroke: Color?, radius: number?)
        table.insert(result, {kind = "rect", x = x * scale, y = y * scale, width = width * scale, height = height * scale,
            radius = radius, fill = fill, stroke = stroke, stroke_width = BORDER * scale, line_cap = "square" :: LineCap} :: Draw)
    end
    local function line(x1: number, y1: number, x2: number, y2: number, color: Color, width: number?)
        table.insert(result, {kind = "path", points = {{x1 * scale, y1 * scale}, {x2 * scale, y2 * scale}},
            stroke = color, stroke_width = (width or BORDER) * scale, line_cap = "square" :: LineCap} :: Draw)
    end
    local function occ(col: number, row: number): number
        return if col < 0 or row < 0 or col >= layout.cols or row >= layout.rows then 0 else occupancy[row * layout.cols + col + 1]
    end
    local function edge(col: number, row: number, side: number, color: Color, width: number?)
        local x, y = 46 + col * SQUARE, 158 + row * SQUARE
        if side == 1 then line(x, y, x + SQUARE, y, color, width)
        elseif side == 2 then line(x, y, x, y + SQUARE, color, width)
        elseif side == 3 then line(x, y + SQUARE, x + SQUARE, y + SQUARE, color, width)
        else line(x + SQUARE, y, x + SQUARE, y + SQUARE, color, width) end
    end
    local function cross(col: number, row: number, color: Color)
        local x, y = 46 + col * SQUARE, 158 + row * SQUARE
        line(x + SQUARE / 2, y, x + SQUARE / 2, y + SQUARE, color)
        line(x, y + SQUARE / 2, x + SQUARE, y + SQUARE / 2, color)
    end
    local neighbors = {{0, -1}, {-1, 0}, {0, 1}, {1, 0}}
    rect(0, 0, layout.cols * SQUARE + 92, layout.rows * SQUARE + 204, layout.background, nil, if raster then 0 else RADIUS)
    -- Color bar: four outlined slots followed by any extra bug colors.
    local bar = view.color_bar()
    local used = 0
    for _, color in bar do if color == "" then break end; used += 1 end
    local left = 40 + (layout.cols * SQUARE + BORDER - (90 * math.max(4, used) + 24)) + 6
    for i = 0, 3 do
        local color = bar[i + 1]
        rect(left + i * 90 + 6, 52, 78, 48, if color and color ~= "" then colors.color(color, true) else BACKGROUND)
        rect(left + i * 90, 46, 90, 60, nil, BLACK)
    end
    for index = 5, #bar do
        if bar[index] ~= "" then rect(left + (index - 1) * 90 + 18, 52, 78, 48, colors.color(bar[index], true)) end
    end
    for row = 0, layout.rows - 1 do
        for col = 0, layout.cols - 1 do
            if not shape.corner(layout, col, row) then rect(46 + col * SQUARE, 158 + row * SQUARE, SQUARE, SQUARE, BACKGROUND, BLACK) end
        end
    end
    -- Resolve and paint each installed footprint, then overdraw its outer border.
    for slot = 1, view.count() do
        local part = view.part(slot)
        local info = if part then assets.part(part.id) else nil
        if not info or not info.color then continue end
        local solid, plus = colors.color(info.color, false), colors.color(info.color, true)
        local cells: {types.Cell} = {}
        for row = 0, layout.rows - 1 do
            for col = 0, layout.cols - 1 do
                if occ(col, row) == slot then
                    table.insert(cells, {col = col, row = row})
                    rect(46 + col * SQUARE, 158 + row * SQUARE, SQUARE, SQUARE, solid)
                end
            end
        end
        for _, cell in cells do
            for side, delta in neighbors do
                if side <= 2 or occ(cell.col + delta[1], cell.row + delta[2]) ~= slot then edge(cell.col, cell.row, side, plus) end
            end
            if not info.solid then cross(cell.col, cell.row, plus) end
        end
    end
    for row = 0, layout.rows - 1 do
        for col = 0, layout.cols - 1 do
            local slot = occ(col, row)
            if slot == 0 then continue end
            for side, delta in neighbors do
                if occ(col + delta[1], row + delta[2]) ~= slot then edge(col, row, side, BLACK) end
            end
        end
    end
    for _, frac in {0.25, 0.75} do
        local y = 158 + (layout.command_line + frac) * SQUARE
        line(46, y, 46 + layout.cols * SQUARE, y, BLACK)
    end
    if layout.out_of_bounds then
        local shade: Color = {0, 0, 0, 128 / 255}
        local band = (layout.rows - 2) * SQUARE + BORDER
        rect(40, 272, 132, band, shade)
        rect(40 + (layout.cols - 1) * SQUARE, 272, 132, band, shade)
        rect(160, 152, band, 132, shade)
        rect(160, 152 + (layout.rows - 1) * SQUARE, band, 132, shade)
    end
    if hover and held then
        local ghost = shape.ghost(held, hover, assets, occupancy)
        local info = assets.part(held.id)
        if ghost and info and info.color then
            local solid = colors.color(info.color, false, 128 / 255)
            local rgb = colors.raw(info.color, true)
            local plus: Color = {math.round((rgb[1] * 128 + 32 * 127) / 255) / 255,
                math.round((rgb[2] * 128 + 32 * 127) / 255) / 255, math.round((rgb[3] * 128 + 32 * 127) / 255) / 255, 1}
            local outline: Color = if ghost.legal then {51 / 255, 217 / 255, 77 / 255, 1} else {230 / 255, 46 / 255, 46 / 255, 1}
            local function contains(cells: {types.Cell}, col: number, row: number): boolean
                for _, cell in cells do if cell.col == col and cell.row == row then return true end end
                return false
            end
            local boundary: {{number}} = {}
            for _, cell in ghost.cells do rect(46 + cell.col * SQUARE, 158 + cell.row * SQUARE, SQUARE, SQUARE, solid) end
            for _, cell in ghost.cells do
                for side, delta in neighbors do
                    local col, row = cell.col + delta[1], cell.row + delta[2]
                    local own = contains(ghost.cells, col, row)
                    if own then
                        if side <= 2 then edge(cell.col, cell.row, side, plus) end
                    elseif not contains(ghost.footprint, col, row) then table.insert(boundary, {cell.col, cell.row, side}) end
                end
                if not info.solid then cross(cell.col, cell.row, plus) end
            end
            for _, mark in boundary do edge(mark[1], mark[2], mark[3], outline) end
        end
    elseif hover then
        local slot = occ(hover.col, hover.row)
        if slot > 0 then
            for row = 0, layout.rows - 1 do
                for col = 0, layout.cols - 1 do
                    if occ(col, row) ~= slot then continue end
                    for side, delta in neighbors do
                        if occ(col + delta[1], row + delta[2]) ~= slot then edge(col, row, side, {1, 1, 1, 1}, math.max(14.4, 2 / scale)) end
                    end
                end
            end
        end
    end
    return result
end

local function drawing(view: types.View, assets: types.Assets, occupancy: {number}): Drawing
    return {width = assets.layout.cols * SQUARE + 92, height = assets.layout.rows * SQUARE + 204,
        commands = commands(view, assets, occupancy, nil, nil, true)}
end

return {geometry = geometry, cell_at = cell_at, commands = commands, scale = SCALE, drawing = drawing}
