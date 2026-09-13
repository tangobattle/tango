--!strict
local types = require("../types")
local grid = require("./grid")

local function view(parts: types.View, assets: types.Assets, occupancy: {number}): Node
    local g = grid.geometry(assets.layout)
    local overlay: {Node} = {{kind = "space", height = g.origin_y}}
    for row = 0, assets.layout.rows - 1 do
        local cells: {Node} = {{kind = "space", width = g.origin_x}}
        for col = 0, assets.layout.cols - 1 do
            local slot = occupancy[row * assets.layout.cols + col + 1]
            local placed = if slot >= 1 then parts.part(slot) else nil
            local info = if placed then assets.part(placed.id) else nil
            local tip: Tooltip? = nil
            if info then
                local lines: {Node} = {{kind = "text", text = info.name or "?"}}
                if info.description then table.insert(lines, {kind = "text", text = info.description, size = "caption" :: TextSize} :: Node) end
                tip = {position = "follow_cursor" :: TooltipPosition, gap = 12, content = {
                    kind = "container" :: "container", padding = 8,
                    appearance = {background = {0, 0, 0, 0.85}, text_color = {1, 1, 1, 1},
                        border_color = {1, 1, 1, 0.2}, border_width = 1, radius = 4},
                    child = {kind = "column" :: "column", children = lines, spacing = 2},
                }}
            end
            table.insert(cells, {kind = "space", width = g.cell, height = g.cell, tooltip = tip} :: Node)
        end
        table.insert(overlay, {kind = "row", children = cells} :: Node)
    end
    return {kind = "column", children = overlay}
end

return {view = view}
