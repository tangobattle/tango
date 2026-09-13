--!strict
local types = require("./types")
export type Part = {read color_id: number, read bitmap: (boolean) -> {boolean}?}

local function materialize(count: number, placement: (number) -> types.Placement?,
    part: (number) -> Part?, board_size: number, bitmap_size: number): ({number}, {number})
    local grid: {number} = table.create(board_size * board_size, 0)
    local colors: {number} = {}
    local center = math.floor(bitmap_size / 2)
    for slot = 1, count do
        local placed = placement(slot)
        if not placed then continue end
        local template = part(placed.id)
        if not template then continue end
        if template.color_id ~= 0 and not table.find(colors, template.color_id) then
            table.insert(colors, template.color_id)
        end
        local bitmap = template.bitmap(placed.compressed) or template.bitmap(false)
        if not bitmap then continue end
        for index, filled in bitmap do
            if not filled then continue end
            local x, y = (index - 1) % bitmap_size, math.floor((index - 1) / bitmap_size)
            for _ = 1, placed.rot % 4 do x, y = bitmap_size - 1 - y, x end
            x += placed.col - center
            y += placed.row - center
            if x >= 0 and x < board_size and y >= 0 and y < board_size then grid[y * board_size + x + 1] = slot end
        end
    end
    return grid, colors
end

return materialize
