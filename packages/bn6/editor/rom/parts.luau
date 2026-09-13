--!strict
local gba = require("@gba")
local offsets = require("./offsets")
local effects = require("../navi/effects")
local source = require("@bn-common/rom/source")
local navicust = require("@bn-common/rom/navicust")
local cards = require("@bn-common/rom/patch_cards")
local navis = require("@bn-common/rom/navis")
export type Part = navicust.Part
export type Effect = cards.Effect
export type Card = cards.Card
export type Navi = navis.Navi
export type Assets = {navicust_part: (number) -> Part?, patch_card: (number) -> Card?, navi: (number) -> Navi?}

local effect_kinds = {
    "max_hp_plus", "max_hp_plus_percent", "max_hp_minus", "max_hp_minus_percent",
    "normal_body", "fire_body", "aqua_body", "elec_body", "wood_body",
    "attack_plus", "attack_minus", "attack_times", "speed_plus", "speed_minus",
    "charge_plus", "charge_minus", "custom_plus", "custom_minus", "mega_folder_plus",
    "mega_folder_minus", "giga_folder_plus", "giga_folder_minus", "super_armor",
    "status_guard", "float_shoes", "air_shoes", "under_shirt",
}
local function effect_kind(id: number): string
    if id < #effect_kinds then return effect_kinds[id + 1] end
    if id == 0x20 then return "triple_buster" end
    if id <= 0x22 then return "b_button_chip" end
    if id <= 0x29 then return "buster_modifier" end
    if id <= 0x56 then return "b_charge_chip" end
    return "other"
end

local function open(mapper: gba.Mapper, definition: offsets.Definition, characters: {string}): Assets
    local reader = source.open(mapper, characters)
    return {
        navicust_part = navicust.open(reader, definition, {count = 188, bitmap_size = 7, effects = effects.for_part, bugs = effects.for_group}),
        patch_card = cards.open(reader, definition, {count = 118, ability_start = 0x16, effect_kind = effect_kind}),
        navi = navis.open(reader, definition, {count = 12, order = definition.navi_order, icon_index = function(id: number): number?
            local offsets = reader.indirect(definition.emblem_icons_offsets_pointer)
            return if offsets and offsets.length >= 12 then offsets.u8(id) else nil
        end}),
    }
end
return {open = open, colors = navicust.colors}
