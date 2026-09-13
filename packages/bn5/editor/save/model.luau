--!strict
local save = require("./init")
local folders = require("@bn-common/save/folders")
local navicust = require("@bn-common/save/navicust")
local auto_battle_data = require("@bn-common/save/auto_battle_data")

export type Chip = folders.Chip
export type Placement = navicust.Placement
export type PatchCard = {id: number, enabled: boolean}
export type NavicustAssets = navicust.Assets
export type RawStats = {base_max_hp: number, current_hp: number, effective_max_hp: number, reg_memory: number}
export type Model = folders.Model & navicust.Model & auto_battle_data.Model & {
    navi: () -> number, set_navi: (number) -> boolean,
    patch_count: () -> number?, set_patch_count: (number) -> boolean,
    patch_card: (number) -> PatchCard?, set_patch_card: (number, PatchCard) -> boolean,
    rebuild_patch_anticheat: () -> boolean,
    raw_stats: () -> RawStats,
    karma: () -> number, set_karma: (number) -> boolean,
    dark_hp_losses: () -> number, set_dark_hp_losses: (number) -> boolean,
}

local function integer(value: number, limit: number): boolean
    return value >= 0 and value < limit and value == math.floor(value)
end
local function index(value: number, count: number): boolean return integer(value - 1, count) end

local function open(bytes: buffer): Model
    local game = save.info(bytes)
    local function address(offset: number): number return save.START + offset end
    local function read(offset: number): number return buffer.readu8(bytes, address(offset)) end
    local function write(offset: number, value: number) buffer.writeu8(bytes, address(offset), value) end
    local function navi(): number return read(0x2941) end
    local function has_navicust(): boolean return navi() == 0 end
    local customization = navicust.open(bytes, {count = 25, num_parts = 192, parts = address(0x4d6c),
        grid = address(0x4d48), grid_bytes = 0x24, colors = address(0x1e8), board_size = 5, bitmap_size = 5}, has_navicust)
    local battle_data = auto_battle_data.open(bytes, {num_chips = 368, primary = address(0x7340),
        secondary = address(0x2340), materialized = address(0x554c)})
    local folder = folders.open(bytes, {count = 3, chips = address(0x2df4), equipped = address(0x52d5), regular = address(0x52d6),
        pack = address(0x2eac), pack_chips = 321, anticheat_source = address(0x1440), anticheat_target = address(0x5cc4),
        anticheat_size = 0x200, anticheat_key = if game.variant == "Protoman" then 0x17 else 0x81})
    return {
        navi = navi,
        set_navi = function(id: number): boolean
            if not integer(id, 13) then return false end
            write(0x2941, id)
            return true
        end,
        folder_count = folder.folder_count,
        equipped_folder = folder.equipped_folder,
        set_equipped_folder = folder.set_equipped_folder,
        chip = folder.chip,
        set_chip = folder.set_chip,
        regular_index = folder.regular_index,
        set_regular_index = folder.set_regular_index,
        pack_count = folder.pack_count,
        set_pack_count = folder.set_pack_count,
        rebuild_chip_anticheat = folder.rebuild_chip_anticheat,
        has_navicust = customization.has_navicust,
        navicust_part = customization.navicust_part,
        set_navicust_part = customization.set_navicust_part,
        materialized = customization.materialized,
        color_bar = customization.color_bar,
        clear_materialized = customization.clear_materialized,
        rebuild_materialized = customization.rebuild_materialized,
        patch_count = function(): number? return if has_navicust() then read(0x79a0) else nil end,
        set_patch_count = function(count: number): boolean
            if not has_navicust() or not integer(count, 256) then return false end
            write(0x79a0, count)
            return true
        end,
        patch_card = function(slot: number): PatchCard?
            if not has_navicust() or not index(slot, read(0x79a0)) then return nil end
            local card = read(0x79d0 + slot - 1)
            return {id = bit32.band(card, 0x7f), enabled = card < 0x80}
        end,
        set_patch_card = function(slot: number, card: PatchCard): boolean
            if not has_navicust() or not index(slot, read(0x79a0)) or not integer(card.id, 128) then return false end
            write(0x79d0 + slot - 1, card.id + (if card.enabled then 0 else 0x80))
            return true
        end,
        rebuild_patch_anticheat = function(): boolean
            if not has_navicust() then return false end
            local key = if game.variant == "Protoman" then 0x43 else 0x8d
            for offset = 0, 0x1ff do write(0x60dc + offset, bit32.bxor(read(0x1220 + offset), key)) end
            return true
        end,
        raw_stats = function(): RawStats
            local offset = 0x52e6 + navi() * 0x60
            return {base_max_hp = buffer.readu16(bytes, address(offset)),
                current_hp = buffer.readu16(bytes, address(offset + 2)),
                effective_max_hp = buffer.readu16(bytes, address(offset + 4)), reg_memory = read(0x52b1)}
        end,
        karma = function(): number return buffer.readu16(bytes, address(0x52ec)) end,
        set_karma = function(value: number): boolean
            if not integer(value, 65536) then return false end
            value = math.min(value, 1000)
            buffer.writeu16(bytes, address(0x52ec), value)
            buffer.writeu32(bytes, address(0x61e0), bit32.bxor(value, buffer.readu32(bytes, address(0x2338))))
            return true
        end,
        dark_hp_losses = function(): number return buffer.readu16(bytes, address(0x29c2)) end,
        set_dark_hp_losses = function(value: number): boolean
            if not integer(value, 65536) then return false end
            buffer.writeu16(bytes, address(0x29c2), value)
            return true
        end,
        chip_use_count = battle_data.chip_use_count,
        set_chip_use_count = battle_data.set_chip_use_count,
        auto_battle_data = battle_data.auto_battle_data,
        clear_auto_battle_data = battle_data.clear_auto_battle_data,
        rebuild_auto_battle_data = battle_data.rebuild_auto_battle_data,
    }
end

return {open = open}
