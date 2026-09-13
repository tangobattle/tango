--!strict
local rules = require("./rules")

export type Chip = rules.ChipInfo & {
    description: string?, element: number, attack: number,
    icon: () -> Raster?, image: () -> Raster?,
}
export type Assets = {
    num_chips: number, chips_have_mb: boolean,
    chip: (number) -> Chip?, chip_is_legal: (number) -> boolean,
    element_icon: (number) -> Raster?,
}
export type Entry = {id: number, code: number, name: string, attack: number, element: number, mb: number}
export type Sort = "id" | "name" | "code" | "attack" | "element" | "mb"
local SORTS: {Sort} = {"id", "name", "code", "attack", "element", "mb"}

local function rule_assets(assets: Assets): rules.Assets
    return {chip = function(id: number): rules.ChipInfo? return assets.chip(id) end,
        chip_is_legal = assets.chip_is_legal}
end

local function entries(assets: Assets, sort: Sort, filter: string): {Entry}
    local result: {Entry} = {}
    local query = tango.lowercase(filter)
    for id = 0, assets.num_chips - 1 do
        if not assets.chip_is_legal(id) then continue end
        local chip = assets.chip(id)
        if not chip or not chip.name then continue end
        if query ~= "" and not string.find(tango.lowercase(chip.name), query, 1, true) then continue end
        for _, code in chip.codes do
            if code < 0 or code > 26 or code ~= math.floor(code) then continue end
            table.insert(result, {id = id, code = code, name = chip.name,
                attack = chip.attack, element = chip.element, mb = chip.mb})
        end
    end
    table.sort(result, function(a: Entry, b: Entry): boolean
        if sort == "name" and a.name ~= b.name then return a.name < b.name end
        if sort == "code" and a.code ~= b.code then return a.code < b.code end
        if sort == "attack" and a.attack ~= b.attack then return a.attack < b.attack end
        if sort == "element" and a.element ~= b.element then return a.element < b.element end
        if sort == "mb" and a.mb ~= b.mb then return a.mb < b.mb end
        return if a.id ~= b.id then a.id < b.id else a.code < b.code
    end)
    return result
end

return {entries = entries, rule_assets = rule_assets, sorts = SORTS}
