--!strict
-- Derive the displayed Navi stats and folder limits from the actual build.
-- Effect catalogs come through assets, so a patch can override them normally.
local model = require("../save/model")
local rom = require("../rom")
local active = require("@bn-common/navi/active_effects")

export type Stats = {
    max_hp: number, attack: number, speed: number, charge: number, b_power_attack: number,
    mega_limit: number, giga_limit: number, reg_memory: number, tag_memory: number,
}

local function derive(save: model.Model, assets: rom.Assets): Stats
    local raw = save.raw_stats()
    if save.navi() ~= 0 then
        return {
            max_hp = raw.effective_max_hp, attack = raw.attack, speed = raw.speed, charge = raw.charge,
            b_power_attack = raw.b_power_attack, mega_limit = raw.mega_limit, giga_limit = raw.giga_limit,
            reg_memory = raw.reg_memory, tag_memory = 60,
        }
    end
    local hp = raw.base_max_hp
    local attack, speed, charge = 0, 0, 0
    local mega, giga = 5, 1
    for _, effect in active.parts(save, assets, 7, nil) do
        local kind, value = effect.kind, effect.value or 0
        if kind == "max_hp" then hp = (hp + value) % 65536
        elseif kind == "attack" then attack = math.min(attack + value, 4)
        elseif kind == "speed" then speed = math.min(speed + value, 4)
        elseif kind == "charge" then charge = math.min(charge + value, 4)
        elseif kind == "attack_max" then attack = 4
        elseif kind == "speed_max" then speed = 4
        elseif kind == "charge_max" then charge = 4 end
    end
    for _, effect in active.parts(save, assets, 7, assets.navicust_command_line) do
        if effect.kind == "mega_limit" then mega += effect.value or 0
        elseif effect.kind == "giga_limit" then giga += effect.value or 0 end
    end
    -- Apply effects in card/record order. Signed Mega limits can recover after
    -- a reduction; Giga limits and buster levels saturate after each operation.
    for _, effect in active.patch_cards(save, assets) do
        local kind, value = effect.kind, effect.parameter
        if kind == "max_hp_plus" then hp = (hp + value * 10) % 65536
        elseif kind == "max_hp_minus" then hp = math.max(0, hp - value * 10)
        elseif kind == "max_hp_plus_percent" then hp = math.floor(hp * (100 + value) / 100) % 65536
        elseif kind == "max_hp_minus_percent" then hp = math.floor(hp * math.max(0, 100 - value) / 100) % 65536
        elseif kind == "attack_plus" then attack = math.min(255, attack + value)
        elseif kind == "attack_minus" then attack = math.max(0, attack - value)
        elseif kind == "attack_times" then attack = math.min(255, attack * value)
        elseif kind == "speed_plus" then speed = math.min(255, speed + value)
        elseif kind == "speed_minus" then speed = math.max(0, speed - value)
        elseif kind == "charge_plus" then charge = math.min(255, charge + value)
        elseif kind == "charge_minus" then charge = math.max(0, charge - value)
        elseif kind == "mega_folder_plus" then mega += value
        elseif kind == "mega_folder_minus" then mega -= value
        elseif kind == "giga_folder_plus" then giga += value
        elseif kind == "giga_folder_minus" then giga = math.max(0, giga - value) end
    end
    return {
        max_hp = hp, attack = math.min(255, attack + 1), speed = math.min(255, speed + 1),
        charge = math.min(255, charge + 1), b_power_attack = 0,
        mega_limit = math.clamp(mega, 0, 10), giga_limit = math.clamp(giga, 0, 10),
        reg_memory = raw.reg_memory, tag_memory = 60,
    }
end

return {
    derive = derive,
    max_copies = function(chip: {mb: number}): number return math.clamp(6 - math.floor(chip.mb / 10), 1, 5) end,
}
