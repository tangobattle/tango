--!strict
local model = require("./model")
local folder = require("../folder/catalog")
export type Sort = "id" | "name" | "used"
export type Entry = {id: number, name: string, used: number, secondary: boolean}
local sorts: {Sort} = {"id", "name", "used"}

local function entries(view: model.View, assets: folder.Assets, sort: Sort, filter: string): {Entry}
    local result: {Entry} = {}
    local query = tango.lowercase(filter)
    for id = 0, assets.num_chips - 1 do
        local info = assets.chip(id)
        if not info or not assets.chip_is_legal(id) then continue end
        if info.class ~= "standard" and info.class ~= "mega" and info.class ~= "giga" and info.class ~= "program_advance" then continue end
        local name = info.name
        if not name or not string.find(name, "%S") then continue end
        if query ~= "" and not string.find(tango.lowercase(name), query, 1, true) then continue end
        table.insert(result, {id = id, name = name, used = view.use_count(id, false), secondary = info.class == "standard"})
    end
    table.sort(result, function(a: Entry, b: Entry): boolean
        if sort == "name" and a.name ~= b.name then return a.name < b.name end
        if sort == "used" and a.used ~= b.used then return a.used > b.used end
        return a.id < b.id
    end)
    return result
end

local function ranking_assets(assets: folder.Assets): model.Assets
    return {num_chips = assets.num_chips, chip_class = function(id: number): string?
        local chip = assets.chip(id)
        return if chip then chip.class else nil
    end}
end

return {entries = entries, sorts = sorts, ranking_assets = ranking_assets}
