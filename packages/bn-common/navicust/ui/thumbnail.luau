--!strict
local types = require("../types")
local shape = require("../shape")
local colors = require("./colors")

local function view(info: types.Info, rot: number, compressed: boolean, dim: boolean): Node
    local bitmap = shape.bitmap(info, compressed)
    if not info.color or not bitmap then return {kind = "space", width = 40, height = 40} end
    local n = bitmap.size
    local cells: {types.Cell} = {}
    local occupied: {[number]: boolean} = {}
    for _, offset in shape.offsets(bitmap, rot) do
        local cell = {col = offset.col + math.floor(n / 2), row = offset.row + math.floor(n / 2)}
        table.insert(cells, cell)
        occupied[cell.row * n + cell.col] = true
    end
    if #cells == 0 then return {kind = "space", width = 40, height = 40} end
    local function own(col: number, row: number): boolean
        return col >= 0 and row >= 0 and col < n and row < n and occupied[row * n + col] == true
    end
    local raster = rot == 0 and compressed
    local pixels = buffer.create(n * n * 64 * 4)
    local commands: {Draw} = {}
    local function rect(x: number, y: number, width: number, height: number, plus: boolean)
        if raster then
            local rgb = colors.raw(info.color :: string, plus)
            local rgba = rgb[1] + rgb[2] * 256 + rgb[3] * 65536 + 255 * 16777216
            for row = y, y + height - 1 do
                for col = x, x + width - 1 do buffer.writeu32(pixels, (row * n * 8 + col) * 4, rgba) end
            end
        else
            table.insert(commands, {kind = "rect", x = x, y = y, width = width, height = height,
                fill = colors.color(info.color :: string, plus, if dim then 89 / 255 else 1)} :: Draw)
        end
    end
    for _, cell in cells do rect(cell.col * 8, cell.row * 8, 8, 8, false) end
    for _, cell in cells do
        local x, y = cell.col * 8, cell.row * 8
        rect(x, y, 8, 1, true)
        rect(x, y, 1, 8, true)
        if not own(cell.col, cell.row + 1) then rect(x, y + 7, 8, 1, true) end
        if not own(cell.col + 1, cell.row) then rect(x + 7, y, 1, 8, true) end
        if not info.solid then rect(x + 4, y, 1, 8, true); rect(x, y + 4, 8, 1, true) end
    end
    if raster then
        return {kind = "image", image = {width = n * 8, height = n * 8, rgba = pixels},
            width = n * 8, height = n * 8, nearest = true, opacity = if dim then 0.35 else 1}
    end
    return {kind = "canvas", canvas_width = n * 8, canvas_height = n * 8, width = n * 8, height = n * 8, commands = commands}
end

return {view = view}
