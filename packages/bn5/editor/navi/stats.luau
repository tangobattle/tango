--!strict
local model = require("../save/model")
local active = require("@bn-common/navi/active_effects")

export type Assets = active.Assets & {read navicust_command_line: number}
export type Stats = {max_hp: number, mega_limit: number?, giga_limit: number?, dark_limit: number?, reg_memory: number?}

local function derive(save: model.Model, assets: Assets): Stats
    local raw = save.raw_stats()
    if save.navi() ~= 0 then return {max_hp = raw.effective_max_hp} end
    local hp, mega, giga = raw.base_max_hp, 5, 1
    for _, effect in active.parts(save, assets, 5, nil) do
        if effect.kind == "max_hp" then hp = (hp + (effect.value or 0)) % 65536 end
    end
    for _, effect in active.parts(save, assets, 5, assets.navicust_command_line) do
        if effect.kind == "mega_limit" then mega += effect.value or 0
        elseif effect.kind == "giga_limit" then giga += effect.value or 0 end
    end
    for _, effect in active.patch_cards(save, assets) do
        local kind, value = effect.kind, effect.parameter
        if kind == "max_hp_plus" then hp = (hp + value * 10) % 65536
        elseif kind == "max_hp_minus" then hp = math.max(0, hp - value * 10)
        elseif kind == "max_hp_plus_percent" then hp = math.floor(hp * (100 + value) / 100) % 65536
        elseif kind == "max_hp_minus_percent" then hp = math.floor(hp * math.max(0, 100 - value) / 100) % 65536
        elseif kind == "mega_folder_plus" then mega += value
        elseif kind == "mega_folder_minus" then mega -= value
        elseif kind == "giga_folder_plus" then giga += value
        elseif kind == "giga_folder_minus" then giga = math.max(0, giga - value) end
    end
    return {max_hp = hp, mega_limit = math.clamp(mega, 0, 10), giga_limit = math.clamp(giga, 0, 10),
        dark_limit = 3, reg_memory = raw.reg_memory}
end

return {
    derive = derive,
    max_copies = function(save: model.Model, chip: {read class: string, read dark: boolean}): number
        if save.navi() ~= 0 then return math.huge end
        if chip.dark or chip.class == "mega" or chip.class == "giga" then return 1 end
        return if chip.class == "standard" then 4 else 0
    end,
}
