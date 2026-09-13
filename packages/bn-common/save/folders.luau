--!strict
-- Shared packed chip storage. Games supply absolute buffer offsets and limits.
export type Chip = {id: number, code: number}
export type Layout = {
    count: number, chips: number, equipped: number, regular: number,
    active_regular: number?, pack: number, pack_chips: number,
    anticheat_source: number, anticheat_target: number, anticheat_size: number, anticheat_key: number,
}
export type Model = {
    folder_count: () -> number, equipped_folder: () -> number, set_equipped_folder: (number) -> boolean,
    chip: (number, number) -> Chip?, set_chip: (number, number, Chip?) -> boolean,
    regular_index: (number) -> number?, set_regular_index: (number, number?) -> boolean,
    pack_count: (number, number) -> number?, set_pack_count: (number, number, number) -> boolean,
    rebuild_chip_anticheat: () -> (),
}
local function integer(value: number, limit: number): boolean
    return value >= 0 and value < limit and value == math.floor(value)
end
local function index(value: number, count: number): boolean return integer(value - 1, count) end

local function open(bytes: buffer, layout: Layout): Model
    local function chip_offset(folder: number, slot: number): number
        return layout.chips + (folder - 1) * 60 + (slot - 1) * 2
    end
    return {
        folder_count = function(): number return layout.count end,
        equipped_folder = function(): number return buffer.readu8(bytes, layout.equipped) + 1 end,
        set_equipped_folder = function(folder: number): boolean
            if not index(folder, layout.count) then return false end
            buffer.writeu8(bytes, layout.equipped, folder - 1)
            if layout.active_regular then
                buffer.writeu8(bytes, layout.active_regular, buffer.readu8(bytes, layout.regular + folder - 1))
            end
            return true
        end,
        chip = function(folder: number, slot: number): Chip?
            if not index(folder, layout.count) or not index(slot, 30) then return nil end
            local raw = buffer.readu16(bytes, chip_offset(folder, slot))
            local code = bit32.rshift(raw, 9)
            return if code <= 26 then {id = bit32.band(raw, 0x1ff), code = code} else nil
        end,
        set_chip = function(folder: number, slot: number, chip: Chip?): boolean
            if not index(folder, layout.count) or not index(slot, 30) then return false end
            if chip and (not integer(chip.id, 512) or not integer(chip.code, 27)) then return false end
            buffer.writeu16(bytes, chip_offset(folder, slot), if chip then chip.id + chip.code * 512 else 0xffff)
            return true
        end,
        regular_index = function(folder: number): number?
            if not index(folder, layout.count) then return nil end
            local slot = buffer.readu8(bytes, layout.regular + folder - 1)
            return if slot < 30 then slot + 1 else nil
        end,
        set_regular_index = function(folder: number, slot: number?): boolean
            if not index(folder, layout.count) or (slot ~= nil and not index(slot, 30)) then return false end
            local raw = if slot then slot - 1 else 0xff
            buffer.writeu8(bytes, layout.regular + folder - 1, raw)
            if layout.active_regular and buffer.readu8(bytes, layout.equipped) == folder - 1 then
                buffer.writeu8(bytes, layout.active_regular, raw)
            end
            return true
        end,
        pack_count = function(id: number, variant: number): number?
            if not integer(id, layout.pack_chips) or not index(variant, 4) then return nil end
            return buffer.readu8(bytes, layout.pack + id * 12 + variant - 1)
        end,
        set_pack_count = function(id: number, variant: number, count: number): boolean
            if not integer(id, layout.pack_chips) or not index(variant, 4) or not integer(count, 256) then return false end
            buffer.writeu8(bytes, layout.pack + id * 12 + variant - 1, count)
            return true
        end,
        rebuild_chip_anticheat = function()
            for offset = 0, layout.anticheat_size - 1 do
                buffer.writeu8(bytes, layout.anticheat_target + offset,
                    bit32.bxor(buffer.readu8(bytes, layout.anticheat_source + offset), layout.anticheat_key))
            end
        end,
    }
end
return {open = open}
