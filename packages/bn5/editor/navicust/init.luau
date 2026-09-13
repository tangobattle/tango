--!strict
local types = require("@bn-common/navicust/types")
local model = require("../save/model")
local rom = require("../rom")
local size = {cols = 5, rows = 5}

local function view(value: model.Model, source: rom.Assets): types.View
    assert(value.has_navicust())
    return {count = function(): number return 25 end, part = value.navicust_part, set_part = value.set_navicust_part,
        rebuild = function() assert(value.rebuild_materialized(source)) end,
        color_bar = function(): {string}
            local result: {string} = {}
            local colors = {"white", "yellow", "pink", "red", "blue", "green"}
            for _, color in value.color_bar() :: {number} do table.insert(result, colors[color] or "") end
            return result
        end}
end

local function assets(source: rom.Assets): types.Assets
    return {num_parts = 192, layout = {cols = size.cols, rows = size.rows, command_line = 2, out_of_bounds = false,
        background = {source.definition.navicust_bg[1] / 255, source.definition.navicust_bg[2] / 255,
            source.definition.navicust_bg[3] / 255, source.definition.navicust_bg[4] / 255}},
        part = function(id: number): types.Info?
            local part = source.navicust_part(id)
            if not part then return nil end
            return {name = part.name, description = part.description, color = part.color, solid = part.solid,
                bitmap = function(compressed: boolean): types.Bitmap?
                    local cells = part.bitmap(compressed)
                    return if cells then {size = 5, cells = cells} else nil
                end}
        end}
end

return {view = view, assets = assets, size = size}
