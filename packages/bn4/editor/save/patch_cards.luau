--!strict
local save = require("./init")
export type Card = {id: number, enabled: boolean}
export type Model = {
    patch_count: () -> number, patch_card: (number) -> Card?,
    set_patch_card: (number, Card?) -> boolean, rebuild_patch_anticheat: () -> (),
}
local function integer(value: number, limit: number): boolean
    return value >= 0 and value < limit and value == math.floor(value)
end
local function open(bytes: buffer): Model
    local game = save.info(bytes)
    local function address(slot: number, enabled: boolean): number
        return save.START + (if enabled then 0x464c else 0x4653) + slot - 1
    end
    return {
        patch_count = function(): number return 6 end,
        patch_card = function(slot: number): Card?
            if not integer(slot - 1, 6) then return nil end
            local id = buffer.readu8(bytes, address(slot, true))
            if id < 134 then return {id = id, enabled = true} end
            id = buffer.readu8(bytes, address(slot, false))
            return if id < 134 then {id = id, enabled = false} else nil
        end,
        set_patch_card = function(slot: number, card: Card?): boolean
            if not integer(slot - 1, 6) then return false end
            if card and (card.id == 0 or not integer(card.id, 134)) then return false end
            buffer.writeu8(bytes, address(slot, true), 0xff)
            buffer.writeu8(bytes, address(slot, false), 0xff)
            if card then buffer.writeu8(bytes, address(slot, card.enabled), card.id) end
            return true
        end,
        rebuild_patch_anticheat = function()
            local mask = if game.variant == "BlueMoon" then 0x31 else 0x43
            for id = 0, 0xff do
                buffer.writeu8(bytes, save.START + 0x5d14 + id,
                    bit32.bxor(buffer.readu8(bytes, save.START + 0x3d0 + id), mask))
            end
        end,
    }
end
return {open = open}
