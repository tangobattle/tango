--!strict
export type Effect = {name: string?, ability: boolean, debuff: boolean}
export type Info = {name: string?, mb: number, effects: {Effect}}
export type Assets = {num_cards: number, card: (number) -> Info?}
export type Entry = {id: number, name: string, mb: number}
export type Sort = "id" | "name" | "mb"
local SORTS: {Sort} = {"id", "name", "mb"}

local function entries(assets: Assets, sort: Sort, filter: string, registered: {[number]: boolean}): {Entry}
    local result: {Entry} = {}
    local query = tango.lowercase(filter)
    for id = 0, assets.num_cards - 1 do
        if registered[id] then continue end
        local info = assets.card(id)
        if not info then continue end
        local name = info.name or "#" .. tostring(id)
        if query ~= "" and not string.find(tango.lowercase(name), query, 1, true) then continue end
        table.insert(result, {id = id, name = name, mb = info.mb})
    end
    table.sort(result, function(a: Entry, b: Entry): boolean
        if sort == "name" and a.name ~= b.name then return a.name < b.name end
        if sort == "mb" and a.mb ~= b.mb then return a.mb < b.mb end
        return a.id < b.id
    end)
    return result
end

return {entries = entries, sorts = SORTS}
