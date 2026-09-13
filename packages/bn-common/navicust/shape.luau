--!strict
local types = require("./types")

local function offsets(bitmap: types.Bitmap, rot: number): {types.Cell}
    local cells: {types.Cell} = {}
    local n = bitmap.size
    for index, filled in bitmap.cells do
        if not filled then continue end
        local col, row = (index - 1) % n, math.floor((index - 1) / n)
        for _ = 1, rot % 4 do col, row = n - 1 - row, col end
        table.insert(cells, {col = col - math.floor(n / 2), row = row - math.floor(n / 2)})
    end
    return cells
end

local function bitmap(info: types.Info, compressed: boolean): types.Bitmap?
    return (if compressed then info.bitmap(true) else nil) or info.bitmap(false)
end

local function materialize(view: types.View, assets: types.Assets): {number}
    local layout = assets.layout
    local grid: {number} = table.create(layout.cols * layout.rows, 0)
    for slot = 1, view.count() do
        local placed = view.part(slot)
        if not placed then continue end
        local info = assets.part(placed.id)
        local source = if info then bitmap(info, placed.compressed) else nil
        if not source then continue end
        for _, cell in offsets(source, placed.rot) do
            local col, row = placed.col + cell.col, placed.row + cell.row
            if col >= 0 and row >= 0 and col < layout.cols and row < layout.rows then
                grid[row * layout.cols + col + 1] = slot
            end
        end
    end
    return grid
end

local function corner(layout: types.Layout, col: number, row: number): boolean
    return layout.out_of_bounds and (col == 0 or col == layout.cols - 1) and (row == 0 or row == layout.rows - 1)
end

local function outer(layout: types.Layout, col: number, row: number): boolean
    return layout.out_of_bounds and (col == 0 or col == layout.cols - 1 or row == 0 or row == layout.rows - 1)
end

export type Ghost = {col: number, row: number, cells: {types.Cell}, footprint: {types.Cell}, legal: boolean}
local function ghost(held: types.Held, at: types.Cell, assets: types.Assets, grid: {number}): Ghost?
    local info = assets.part(held.id)
    local source = if info then bitmap(info, held.compressed) else nil
    if not source then return nil end
    local layout = assets.layout
    local col, row = at.col - held.grab_col, at.row - held.grab_row
    local result: Ghost = {col = col, row = row, cells = {}, footprint = {}, legal = col >= 0 and row >= 0 and col < 256 and row < 256}
    local interior = false
    for _, offset in offsets(source, held.rot) do
        local cell = {col = col + offset.col, row = row + offset.row}
        table.insert(result.footprint, cell)
        if cell.col < 0 or cell.row < 0 or cell.col >= layout.cols or cell.row >= layout.rows then
            result.legal = false
            continue
        end
        table.insert(result.cells, cell)
        if corner(layout, cell.col, cell.row) or grid[cell.row * layout.cols + cell.col + 1] ~= 0 then result.legal = false end
        if not outer(layout, cell.col, cell.row) then interior = true end
    end
    if layout.out_of_bounds and not interior then result.legal = false end
    return result
end

local function compressible(info: types.Info): boolean
    local compressed, plain = info.bitmap(true), info.bitmap(false)
    if not compressed or not plain then return false end
    if compressed.size ~= plain.size then return true end
    for index, filled in compressed.cells do if filled ~= plain.cells[index] then return true end end
    return false
end

return {offsets = offsets, bitmap = bitmap, materialize = materialize, corner = corner, outer = outer,
    ghost = ghost, compressible = compressible}
