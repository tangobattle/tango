--!strict
local active = require("@bn-common/navi/active_effects")
local model = require("../save/model")
local catalog = require("../patch_cards/catalog")

export type Assets = active.PartsAssets & {
    read navicust_command_line: number,
    read patch_card: (number) -> {read effect: catalog.Effect}?,
}
export type Stats = {max_hp: number, mega_limit: number, giga_limit: number, reg_memory: number}

local function derive(save: model.Model, assets: Assets): Stats
    local raw = save.raw_stats()
    local hp, mega, giga = raw.base_max_hp, 5, 1
    for _, effect in active.parts(save, assets, 5, nil) do
        if effect.kind == "max_hp" then hp = (hp + (effect.value or 0)) % 65536 end
    end
    for _, effect in active.parts(save, assets, 5, assets.navicust_command_line) do
        if effect.kind == "mega_limit" then mega += effect.value or 0
        elseif effect.kind == "giga_limit" then giga += effect.value or 0 end
    end
    for slot = 1, save.patch_count() do
        local card = save.patch_card(slot)
        if not card or not card.enabled then continue end
        local info = assets.patch_card(card.id)
        if not info then continue end
        -- The game applies enabled cards even in the wrong slot. Report that
        -- binding separately; do not change the displayed effective stats.
        local effect = info.effect
        if effect.kind == "max_hp" then hp = (hp + effect.value) % 65536
        elseif effect.kind == "mega_folder" then mega += effect.value
        elseif effect.kind == "giga_folder" then giga += effect.value end
    end
    return {max_hp = hp, mega_limit = math.clamp(mega, 0, 10), giga_limit = math.clamp(giga, 0, 10), reg_memory = raw.reg_memory}
end

return {
    derive = derive,
    max_copies = function(chip: {read class: string, read dark: boolean}): number
        if chip.dark then return 0 end
        if chip.class == "mega" or chip.class == "giga" then return 1 end
        return if chip.class == "standard" then 4 else 0
    end,
}
