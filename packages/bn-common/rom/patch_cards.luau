--!strict
local source = require("./source")
export type Effect = {
    id: number, kind: string, name: string?, parameter: number, ability: boolean, debuff: boolean,
}
export type Card = {id: number, name: string?, mb: number, effects: {Effect}}
export type Offsets = {
    read patch_card_data: number, read patch_card_names_pointer: number, read patch_card_details_names_pointer: number,
}
export type Format = {count: number, ability_start: number, effect_kind: (number) -> string}
local function open(reader: source.Source, definition: Offsets, format: Format): (number) -> Card?
    local cards: {[number]: Card} = {}
    return function(id: number): Card?
        if id < 1 or id >= format.count or id ~= math.floor(id) or definition.patch_card_data == 0 then return nil end
        if cards[id] then return cards[id] end
        local raw = reader.region(definition.patch_card_data)
        if not raw then return nil end
        local start, finish = raw.u16(id * 2), raw.u16((id + 1) * 2)
        if not start or not finish or finish < start + 3 or (finish - start) % 3 ~= 0 or finish > raw.length then return nil end
        local effects: {Effect} = {}
        for offset = start + 3, finish - 1, 3 do
            local effect_id = raw.u8(offset) :: number
            local parameter = raw.u8(offset + 1) :: number
            local effect: Effect = {
                id = effect_id, kind = format.effect_kind(effect_id), parameter = parameter,
                ability = effect_id >= format.ability_start, debuff = raw.u8(offset + 2) == 1,
                name = reader.text(definition.patch_card_details_names_pointer, effect_id + 1,
                    function(kind: string, variable: number): string?
                        if kind ~= "variable" or variable ~= 1 then return "" end
                        return tostring(parameter * (if effect_id == 0 or effect_id == 2 then 10 else 1))
                    end),
            }
            table.insert(effects, effect)
        end
        local result: Card = {
            id = id, mb = raw.u8(start + 1) :: number, effects = effects,
            name = reader.text(definition.patch_card_names_pointer, id + 1, nil),
        }
        cards[id] = result
        return result
    end
end
return {open = open}
