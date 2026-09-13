--!strict
local save = require("./init")
local folders = require("./folders")
local navicust = require("./navicust")
local cards = require("./patch_cards")
local auto_battle_data = require("./auto_battle_data")

export type RawStats = {base_max_hp: number, reg_memory: number}
export type Model = folders.Model & navicust.Model & cards.Model & auto_battle_data.Model & {
    raw_stats: () -> RawStats,
}

local function open(bytes: buffer): Model
    local folder = folders.open(bytes)
    local customization = navicust.open(bytes)
    local patch = cards.open(bytes)
    local battle_data = auto_battle_data.open(bytes)
    return {
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
        patch_count = patch.patch_count,
        patch_card = patch.patch_card,
        set_patch_card = patch.set_patch_card,
        rebuild_patch_anticheat = patch.rebuild_patch_anticheat,
        chip_use_count = battle_data.chip_use_count,
        set_chip_use_count = battle_data.set_chip_use_count,
        auto_battle_data = battle_data.auto_battle_data,
        clear_auto_battle_data = battle_data.clear_auto_battle_data,
        rebuild_auto_battle_data = battle_data.rebuild_auto_battle_data,
        raw_stats = function(): RawStats
            return {base_max_hp = buffer.readu16(bytes, save.START + 0x21ca),
                reg_memory = (buffer.readu8(bytes, save.START + 0x2148) + 4) % 256}
        end,
    }
end
return {open = open}
