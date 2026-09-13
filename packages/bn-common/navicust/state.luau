--!strict
local types = require("./types")
local catalog = require("./catalog")
local edit = require("./edit")
local shape = require("./shape")

export type Selection = {filter: string, sort: catalog.Sort, held: types.Held?, hover: types.Cell?}
export type Command = {kind: "pick", id: number} | {kind: "rotate", id: number} | {kind: "compress", id: number}
    | {kind: "rotate_held"} | {kind: "clear_held"} | {kind: "clear"}
    | {kind: "cell", col: number, row: number} | {kind: "hover", cell: types.Cell?}
    | {kind: "filter", value: string} | {kind: "sort", value: catalog.Sort}

local function number(state: ViewState, key: string, fallback: number): number
    return tonumber(state["navicust." .. key] or "") or fallback
end

local function orientation(state: ViewState, id: number): (number, boolean)
    local key = "part." .. tostring(id)
    return number(state, key .. ".rot", 0) % 4, state["navicust." .. key .. ".compressed"] ~= "false"
end

local function orient(state: ViewState, id: number, rot: number, compressed: boolean)
    local key = "navicust.part." .. tostring(id)
    state[key .. ".rot"] = tostring(rot)
    state[key .. ".compressed"] = tostring(compressed)
end

local function set_held(state: ViewState, held: types.Held?)
    state["navicust.held.id"] = if held then tostring(held.id) else nil
    state["navicust.held.rot"] = if held then tostring(held.rot) else nil
    state["navicust.held.compressed"] = if held then tostring(held.compressed) else nil
    state["navicust.held.grab_col"] = if held then tostring(held.grab_col) else nil
    state["navicust.held.grab_row"] = if held then tostring(held.grab_row) else nil
end

local function read(state: ViewState): Selection
    local held: types.Held? = nil
    if state["navicust.held.id"] then
        held = {id = number(state, "held.id", 0), rot = number(state, "held.rot", 0),
            compressed = state["navicust.held.compressed"] ~= "false",
            grab_col = number(state, "held.grab_col", 0), grab_row = number(state, "held.grab_row", 0)}
    end
    local hover: types.Cell? = if state["navicust.hover.col"] then
        {col = number(state, "hover.col", 0), row = number(state, "hover.row", 0)} else nil
    local sort = state["navicust.sort"]
    return {filter = state["navicust.filter"] or "", sort = if sort == "name" then "name" else if sort == "color" then "color" else "id",
        held = held, hover = hover}
end

local function rotate(held: types.Held)
    held.rot = (held.rot + 1) % 4
    held.grab_col, held.grab_row = -held.grab_row, held.grab_col
end

local function apply(state: ViewState, view: types.View, assets: types.Assets, command: Command)
    local held = read(state).held
    if command.kind == "pick" then
        if held and held.id == command.id then set_held(state, nil); return end
        if (edit.counts(view)[command.id] or 0) >= edit.MAX_COPIES or not assets.part(command.id) then return end
        local rot, compressed = orientation(state, command.id)
        set_held(state, {id = command.id, rot = rot, compressed = compressed, grab_col = 0, grab_row = 0})
    elseif command.kind == "rotate" or command.kind == "compress" then
        local id, kind = command.id, command.kind
        local info = assets.part(id)
        if not info then return end
        local rot, compressed = orientation(state, id)
        if kind == "rotate" then rot = (rot + 1) % 4
        else
            if not shape.compressible(info) then return end
            compressed = not compressed
        end
        orient(state, id, rot, compressed)
        if held and held.id == id then
            if kind == "rotate" then rotate(held)
            else held.compressed = compressed; held.grab_col, held.grab_row = 0, 0 end
            set_held(state, held)
        end
    elseif command.kind == "rotate_held" then
        if held then rotate(held); set_held(state, held); orient(state, held.id, held.rot, held.compressed) end
    elseif command.kind == "clear_held" then set_held(state, nil)
    elseif command.kind == "clear" then set_held(state, nil); edit.apply(view, {kind = "clear"})
    elseif command.kind == "filter" then state["navicust.filter"] = command.value
    elseif command.kind == "sort" then state["navicust.sort"] = command.value
    elseif command.kind == "hover" then
        state["navicust.hover.col"] = if command.cell then tostring(command.cell.col) else nil
        state["navicust.hover.row"] = if command.cell then tostring(command.cell.row) else nil
    elseif command.kind == "cell" then
        local col, row = command.col, command.row
        if col < 0 or row < 0 or col >= assets.layout.cols or row >= assets.layout.rows
            or col ~= math.floor(col) or row ~= math.floor(row) then return end
        local grid = shape.materialize(view, assets)
        if held then
            local ghost = shape.ghost(held, {col = col, row = row}, assets, grid)
            if not ghost or not ghost.legal then return end
            edit.apply(view, {kind = "add", part = {id = held.id, rot = held.rot, compressed = held.compressed, col = ghost.col, row = ghost.row}})
            set_held(state, nil)
        else
            local slot = grid[row * assets.layout.cols + col + 1]
            local part = if slot >= 1 then view.part(slot) else nil
            if not part then return end
            set_held(state, {id = part.id, rot = part.rot, compressed = part.compressed,
                grab_col = col - part.col, grab_row = row - part.row})
            orient(state, part.id, part.rot, part.compressed)
            edit.apply(view, {kind = "remove", slot = slot})
        end
    end
end

return {orientation = orientation, read = read, apply = apply}
