--!strict
local data = require("../auto_battle_data/model")
export type Slot = data.Slot
export type Assets = data.Assets
export type Layout = {num_chips: number, primary: number, secondary: number, materialized: number}
export type Model = {
    chip_use_count: (number, boolean?) -> number?, set_chip_use_count: (number, number, boolean?) -> boolean,
    auto_battle_data: () -> {Slot}, clear_auto_battle_data: () -> (),
    rebuild_auto_battle_data: (Assets) -> (),
}
local function integer(value: number, limit: number): boolean
    return value >= 0 and value < limit and value == math.floor(value)
end
local function open(bytes: buffer, layout: Layout): Model
    local function use_count(id: number, secondary: boolean?): number?
        if not integer(id, layout.num_chips) then return nil end
        return buffer.readu16(bytes, (if secondary then layout.secondary else layout.primary) + id * 2)
    end
    return {
        chip_use_count = use_count,
        set_chip_use_count = function(id: number, count: number, secondary: boolean?): boolean
            if not integer(id, layout.num_chips) or not integer(count, 65536) then return false end
            buffer.writeu16(bytes, (if secondary then layout.secondary else layout.primary) + id * 2, count)
            return true
        end,
        auto_battle_data = function(): {Slot}
            local slots: {Slot} = {}
            for slot = 1, 42 do
                local id = buffer.readu16(bytes, layout.materialized + (slot - 1) * 2)
                slots[slot] = {id = if id == 0xffff then nil else id}
            end
            return slots
        end,
        clear_auto_battle_data = function() buffer.fill(bytes, layout.materialized, 0xff, 84) end,
        rebuild_auto_battle_data = function(assets: Assets)
            local slots = data.materialize({use_count = function(id: number, secondary: boolean): number
                return use_count(id, secondary) or 0
            end}, assets)
            for slot, chip in slots do buffer.writeu16(bytes, layout.materialized + (slot - 1) * 2, chip.id or 0xffff) end
        end,
    }
end
return {open = open}
