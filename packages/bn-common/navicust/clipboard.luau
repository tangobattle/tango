--!strict
local types = require("./types")
local grid = require("./ui/grid")

local function text(parts: types.View, assets: types.Assets, style_name: string?): string
    local solid: {string}, plus: {string} = {}, {}
    for slot = 1, parts.count() do
        local part = parts.part(slot)
        local info = if part then assets.part(part.id) else nil
        if not part or not info then continue end
        table.insert(if info.solid then solid else plus, info.name or ("#" .. tostring(part.id)))
    end
    local output: {string} = if style_name then {style_name .. "\n"} else {}
    for index = 1, math.max(#solid, #plus) do
        table.insert(output, (solid[index] or "") .. "\t" .. (plus[index] or "") .. "\n")
    end
    return table.concat(output)
end

return {text = text, image = grid.drawing}
