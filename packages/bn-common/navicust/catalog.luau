--!strict
local types = require("./types")
local text = require("@editor-common/text")
export type Sort = "id" | "name" | "color"
export type Entry = {id: number, name: string, description: string?, color_rank: number}
local colors = {"white", "yellow", "pink", "red", "blue", "green", "orange", "purple", "gray"}

local function entries(assets: types.Assets, sort: Sort, filter: string): {Entry}
    local result: {Entry} = {}
    local per_name: {[string]: number} = {}
    filter = tango.lowercase(filter)
    for id = 0, assets.num_parts - 1 do
        local info = assets.part(id)
        if not info or not info.color or not info.name or text.is_blank(info.name) then continue end
        local bitmap = info.bitmap(false)
        if not bitmap or not table.find(bitmap.cells, true) then continue end
        if filter ~= "" and not string.find(tango.lowercase(info.name), filter, 1, true) then continue end
        local count = per_name[info.name] or 0
        if count >= 9 then continue end
        per_name[info.name] = count + 1
        table.insert(result, {id = id, name = info.name, description = info.description,
            color_rank = table.find(colors, info.color) or #colors + 1})
    end
    if sort ~= "id" then
        table.sort(result, function(a: Entry, b: Entry): boolean
            if sort == "name" and a.name ~= b.name then return a.name < b.name end
            if sort == "color" and a.color_rank ~= b.color_rank then return a.color_rank < b.color_rank end
            return a.id < b.id
        end)
    end
    return result
end

return {entries = entries, sorts = {"id", "name", "color"} :: {Sort}}
