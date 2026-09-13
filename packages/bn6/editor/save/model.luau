--!strict
-- Cartridge save operations, in the cartridge's original regional layout.
-- Collection indices are 1-based. Byte offsets and encoded IDs retain their
-- format-defined values. Rejected mutations return false without writing.
local save = require("./init")
local rom = require("../rom")
local link_navis = require("../navi/templates")
local materialize = require("@bn-common/navicust/materialize")

export type Chip = {id: number, code: number}
export type Placement = {id: number, col: number, row: number, rot: number, compressed: boolean}
export type PatchCard = {id: number, enabled: boolean}
export type RawStats = {
    attack: number, speed: number, charge: number, b_power_attack: number,
    reg_memory: number, custom_level: number, mega_limit: number, giga_limit: number,
    base_max_hp: number, current_hp: number, effective_max_hp: number,
}
export type Model = {
    navi: () -> number, set_navi: (number) -> boolean,
    folder_count: () -> number, equipped_folder: () -> number,
    set_equipped_folder: (number) -> boolean,
    chip: (number, number) -> Chip?, set_chip: (number, number, Chip?) -> boolean,
    regular_index: (number) -> number?, set_regular_index: (number, number?) -> boolean,
    tag_indexes: (number) -> {number}?, set_tag_indexes: (number, {number}?) -> boolean,
    pack_count: (number, number) -> number?, set_pack_count: (number, number, number) -> boolean,
    rebuild_chip_anticheat: () -> (),
    has_navicust: () -> boolean, navicust_part: (number) -> Placement?,
    set_navicust_part: (number, Placement?) -> boolean,
    materialized: () -> {number}?, color_bar: () -> {number}?,
    clear_materialized: () -> boolean, rebuild_materialized: (rom.Assets) -> boolean,
    patch_count: () -> number?, set_patch_count: (number) -> boolean,
    patch_card: (number) -> PatchCard?, set_patch_card: (number, PatchCard) -> boolean,
    rebuild_patch_anticheat: () -> boolean,
    -- Read the operative stats block; derived values belong in navi_stats.luau.
    raw_stats: () -> RawStats,
}

local function integer(value: number, limit: number): boolean
    return value >= 0 and value < limit and value == math.floor(value)
end

local function index(value: number, count: number): boolean
    return integer(value - 1, count)
end

local function open(bytes: buffer): Model
    local game = save.info(bytes)
    local function address(offset: number): number
        -- Only this interval shifts. In particular, the patch-card anticheat
        -- mirror crosses its end, so shifting that entire mirror is wrong.
        return save.START + offset - (if game.region == "JP" and offset >= 0x414c and offset < 0x513c then 0x40 else 0)
    end
    local function read(offset: number): number return buffer.readu8(bytes, address(offset)) end
    local function write(offset: number, value: number) buffer.writeu8(bytes, address(offset), value) end
    local function navi(): number
        return if bit32.band(read(0x1cb4), 0x10) ~= 0 then read(0x1b81) else 0
    end
    local function stats(): number return 0x47cc + (if navi() == 0 then 0 else 0x64) end
    local function folder_count(): number return read(0x1c09) end
    local function valid_folder(folder: number): boolean return index(folder, math.min(folder_count(), 3)) end
    local function chip_offset(folder: number, slot: number): number return 0x2178 + (folder - 1) * 60 + (slot - 1) * 2 end
    local function has_navicust(): boolean return navi() == 0 end
    local function has_patch_cards(): boolean return has_navicust() and game.region == "JP" end
    local function part(slot: number): Placement?
        if not has_navicust() or not index(slot, 25) then return nil end
        local offset = 0x4190 + (slot - 1) * 8
        local id = read(offset)
        if id == 0 then return nil end
        return {id = id, col = read(offset + 3), row = read(offset + 4), rot = read(offset + 5), compressed = read(offset + 6) ~= 0}
    end
    local function clear_materialized(): boolean
        if not has_navicust() then return false end
        buffer.fill(bytes, address(0x414c), 0, 0x44)
        buffer.fill(bytes, address(0x90), 0, 6)
        return true
    end

    return {
        navi = navi,
        set_navi = function(id: number): boolean
            if not integer(id, 12) then return false end
            if id == 0 then
                write(0x1b81, 0)
                write(0x1cb4, bit32.band(read(0x1cb4), 0xef))
                buffer.writeu16(bytes, address(0x1c34), 0)
                -- The previous link-Navi block remains as equip residue.
                return true
            end
            local template = link_navis.stats(id)
            if not template then return false end
            buffer.copy(bytes, address(0x4830), template)
            write(0x1b81, id)
            write(0x1cb4, bit32.bor(read(0x1cb4), 0x10))
            buffer.writeu16(bytes, address(0x1c34), 335 + 15 * id)
            return true
        end,
        folder_count = folder_count,
        equipped_folder = function(): number return read(stats() + 0x2d) + 1 end,
        set_equipped_folder = function(folder: number): boolean
            if not valid_folder(folder) then return false end
            write(stats() + 0x2d, folder - 1)
            return true
        end,
        chip = function(folder: number, slot: number): Chip?
            if not valid_folder(folder) or not index(slot, 30) then return nil end
            local value = buffer.readu16(bytes, address(chip_offset(folder, slot)))
            local code = bit32.rshift(value, 9)
            if code > 26 then return nil end
            return {id = bit32.band(value, 0x1ff), code = code}
        end,
        set_chip = function(folder: number, slot: number, chip: Chip?): boolean
            if not valid_folder(folder) or not index(slot, 30) then return false end
            if chip and (not integer(chip.id, 512) or not integer(chip.code, 27)) then return false end
            local value = if chip then chip.id + chip.code * 512 else 0xffff
            buffer.writeu16(bytes, address(chip_offset(folder, slot)), value)
            return true
        end,
        regular_index = function(folder: number): number?
            if not valid_folder(folder) then return nil end
            local value = read(stats() + 0x2e + folder - 1)
            return if value < 30 then value + 1 else nil
        end,
        set_regular_index = function(folder: number, slot: number?): boolean
            if not valid_folder(folder) or (slot ~= nil and not index(slot, 30)) then return false end
            local offset = stats()
            -- Even a partially unset raw Tag pair reserves its other slot.
            if slot ~= nil and (read(offset + 0x56 + (folder - 1) * 2) == slot - 1 or read(offset + 0x57 + (folder - 1) * 2) == slot - 1) then
                return false
            end
            write(offset + 0x2e + folder - 1, if slot then slot - 1 else 0xff)
            return true
        end,
        tag_indexes = function(folder: number): {number}?
            if not valid_folder(folder) then return nil end
            local offset = stats() + 0x56 + (folder - 1) * 2
            local a, b = read(offset), read(offset + 1)
            return if a == 0xff or b == 0xff then nil else {a + 1, b + 1}
        end,
        set_tag_indexes = function(folder: number, slots: {number}?): boolean
            if not valid_folder(folder) then return false end
            local offset = stats()
            if slots then
                if #slots ~= 2 or not index(slots[1], 30) or not index(slots[2], 30) then return false end
                local regular = read(offset + 0x2e + folder - 1)
                if regular == slots[1] - 1 or regular == slots[2] - 1 then return false end
            end
            write(offset + 0x56 + (folder - 1) * 2, if slots then slots[1] - 1 else 0xff)
            write(offset + 0x57 + (folder - 1) * 2, if slots then slots[2] - 1 else 0xff)
            return true
        end,
        pack_count = function(id: number, variant: number): number?
            if not integer(id, 321) or not index(variant, 4) then return nil end
            return read(0x2230 + id * 12 + variant - 1)
        end,
        set_pack_count = function(id: number, variant: number, count: number): boolean
            if not integer(id, 321) or not index(variant, 4) or not integer(count, 256) then return false end
            write(0x2230 + id * 12 + variant - 1, count)
            return true
        end,
        rebuild_chip_anticheat = function() save.rebuild_anticheat(bytes) end,
        has_navicust = has_navicust,
        navicust_part = part,
        set_navicust_part = function(slot: number, value: Placement?): boolean
            if not has_navicust() or not index(slot, 25) then return false end
            if value and (not integer(value.id, 188) or not integer(value.col, 256)
                or not integer(value.row, 256) or not integer(value.rot, 256)) then return false end
            local offset = 0x4190 + (slot - 1) * 8
            buffer.fill(bytes, address(offset), 0, 8)
            if value then
                write(offset, value.id)
                write(offset + 3, value.col)
                write(offset + 4, value.row)
                write(offset + 5, value.rot)
                write(offset + 6, if value.compressed then 1 else 0)
            end
            return true
        end,
        materialized = function(): {number}?
            if not has_navicust() then return nil end
            local grid: {number} = {}
            for i = 1, 49 do grid[i] = read(0x414c + i - 1) end
            return grid -- row-major, 0 = blank; otherwise the 1-based placement slot
        end,
        color_bar = function(): {number}?
            if not has_navicust() then return nil end
            local colors: {number} = {}
            for i = 1, 6 do
                local value = read(0x90 + i - 1)
                colors[i] = if value <= 6 then value else 0
            end
            return colors
        end,
        clear_materialized = clear_materialized,
        rebuild_materialized = function(assets: rom.Assets): boolean
            if not has_navicust() then return false end
            local grid, colors = materialize(25, part, assets.navicust_part, 7, 7)
            clear_materialized()
            for index, slot in grid do write(0x414c + index - 1, slot) end
            for index = 1, math.min(#colors, 6) do write(0x90 + index - 1, colors[index]) end
            return true
        end,
        patch_count = function(): number? return if has_patch_cards() then read(0x65f0) else nil end,
        set_patch_count = function(count: number): boolean
            -- The allocated list ends at the end of the save region. Native
            -- u8 wrapping and out-of-range writes are not valid edit requests.
            if not has_patch_cards() or not integer(count, 241) then return false end
            write(0x65f0, count)
            return true
        end,
        patch_card = function(slot: number): PatchCard?
            if not has_patch_cards() or not index(slot, math.min(read(0x65f0), 240)) then return nil end
            local value = read(0x6620 + slot - 1)
            return {id = bit32.band(value, 0x7f), enabled = value < 0x80}
        end,
        set_patch_card = function(slot: number, card: PatchCard): boolean
            if not has_patch_cards() or not index(slot, math.min(read(0x65f0), 240)) or not integer(card.id, 128) then
                return false
            end
            write(0x6620 + slot - 1, card.id + (if card.enabled then 0 else 0x80))
            return true
        end,
        rebuild_patch_anticheat = function(): boolean
            if not has_patch_cards() then return false end
            local key = if game.variant == "Gregar" then 0x43 else 0x8d
            for offset = 0, 0x1ff do write(0x5038 + offset, bit32.bxor(read(0x0670 + offset), key)) end
            return true
        end,
        raw_stats = function(): RawStats
            local offset = stats()
            return {
                attack = read(offset + 1), speed = read(offset + 2), charge = read(offset + 3),
                b_power_attack = read(offset + 5), reg_memory = read(offset + 9),
                custom_level = read(offset + 0xa), mega_limit = read(offset + 0xb), giga_limit = read(offset + 0xc),
                base_max_hp = buffer.readu16(bytes, address(offset + 0x3e)),
                current_hp = buffer.readu16(bytes, address(offset + 0x40)),
                effective_max_hp = buffer.readu16(bytes, address(offset + 0x42)),
            }
        end,
    }
end

return {open = open}
