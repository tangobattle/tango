--!strict
local catalog = require("./catalog")
local rules = require("./rules")
local reference = require("../save/fixtures/catalog")

local function normalized(value: string): string return (string.gsub(value, "_", "")) end
local function effect_text(effect: catalog.Effect): string
    local kind = normalized(effect.kind)
    if effect.kind == "max_hp" or effect.kind == "buster_attack" or effect.kind == "custom_slots"
        or effect.kind == "mega_folder" or effect.kind == "giga_folder" then
        return kind .. "(" .. tostring(effect.value) .. ")"
    elseif effect.kind == "b_button" or effect.kind == "b_charge" or effect.kind == "b_left" then
        return kind .. "(" .. normalized(effect.shot) .. ")"
    elseif effect.kind == "panel_step" then return kind .. "(" .. normalized(effect.panel) .. ")"
    elseif effect.kind == "aura" then return kind .. "(" .. normalized(effect.aura) .. ")"
    elseif effect.kind == "soul" then return kind .. "(" .. normalized(effect.soul) .. ")"
    elseif effect.kind == "color" or effect.kind == "pet_menu" then return kind .. "(" .. normalized(effect.color) .. ")" end
    return kind
end

return function()
    local regions: {"JP" | "US"} = {"JP", "US"}
    for _, region in regions do
        local entries = if region == "JP" then reference.cards.JP else reference.cards.US
        for id = 1, 133 do
            local expected = entries[id]
            local card = catalog.get(id, region)
            assert(card and card.name == expected.name and card.slot == expected.slot, tostring(id))
            assert(effect_text(card.effect) == expected.effect, tostring(id))
            assert(#card.bugs == #expected.bugs)
            for index, bug in card.bugs do
                local actual = normalized(bug.kind) .. (if bug.kind == "hp" then "(" .. tostring(bug.value) .. ")" else "")
                assert(actual == expected.bugs[index], tostring(id))
            end
        end
    end
    for _, id in {-1, 0, 0.5, 134, 0/0, math.huge} do assert(catalog.get(id, "US") == nil) end

    -- An already-created consumer observes a dependent package's method replacement.
    local assets = {patch_card = function(id: number): catalog.Card? return catalog.get(id, "US") end}
    local view = {patch_count = function(): number return 1 end,
        patch_card = function(_slot: number): {id: number, enabled: boolean}? return {id = 7, enabled = false} end}
    local before = rules.violations(view, assets)
    assert(#before == 1 and before[1].kind == "wrong_slot" and before[1].expected_slot == 3 and not before[1].enabled)
    local original = assets.patch_card
    assets.patch_card = function(id: number): catalog.Card?
        local card = original(id)
        if not card then return nil end
        local copy = table.clone(card)
        copy.slot = 1
        return copy
    end
    assert(#rules.violations(view, assets) == 0)
    assets.patch_card = function(_id: number): catalog.Card? return nil end
    local missing = rules.violations(view, assets)
    assert(#missing == 1 and missing[1].kind == "not_in_catalog")
end
