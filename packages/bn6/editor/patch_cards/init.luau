--!strict
local catalog = require("@bn-common/patch_cards/catalog")
local edit = require("@bn-common/patch_cards/edit")
local model = require("../save/model")
local rom = require("../rom")

local function view(value: model.Model): edit.View
    assert(value.patch_count() ~= nil)
    return {
        count = function(): number return value.patch_count() :: number end,
        card = value.patch_card, set_count = value.set_patch_count, set_card = value.set_patch_card,
        rebuild_anticheat = function() assert(value.rebuild_patch_anticheat()) end,
    }
end

local function assets(source: rom.Assets): catalog.Assets
    return {num_cards = 118, card = function(id: number): catalog.Info?
        local card = source.patch_card(id)
        if not card then return nil end
        local effects: {catalog.Effect} = {}
        for _, effect in card.effects do
            table.insert(effects, {name = effect.name, ability = effect.ability, debuff = effect.debuff})
        end
        return {name = card.name, mb = card.mb, effects = effects}
    end}
end

return {view = view, assets = assets}
