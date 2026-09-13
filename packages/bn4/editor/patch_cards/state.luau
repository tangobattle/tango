--!strict
local cards = require("../save/patch_cards")
local catalog = require("./catalog")
export type Assets = {read patch_card: (number) -> catalog.Card?}

local function update(value: cards.Model, assets: Assets, action: Action): boolean
    if action.kind == "activate" and action.id == "patch_cards.clear" then
        for slot = 1, value.patch_count() do value.set_patch_card(slot, nil) end
    elseif action.kind == "change" then
        local slot = tonumber(string.match(action.id, "^patch_cards%.slot:(%d+)$") or "")
        if not slot or slot < 1 or slot > value.patch_count() then return false end
        if action.value == "none" then value.set_patch_card(slot, nil)
        else
            local id = tonumber(action.value)
            local info = if id then assets.patch_card(id) else nil
            if not info or info.slot ~= slot or not value.set_patch_card(slot, {id = id :: number, enabled = true}) then return false end
        end
    elseif action.kind == "activate" then
        local slot = tonumber(string.match(action.id, "^patch_cards%.toggle:(%d+)$") or "")
        local card = if slot then value.patch_card(slot) else nil
        if not card then return false end
        if not value.set_patch_card(slot :: number, {id = card.id, enabled = not card.enabled}) then return false end
    else return false end
    value.rebuild_patch_anticheat()
    return true
end

local function as_text(value: cards.Model, assets: Assets): string
    local labels = require("./labels")
    local lines: {string} = {}
    for slot = 1, value.patch_count() do
        local card = value.patch_card(slot)
        if card and card.enabled then table.insert(lines, labels.slot(slot) .. "\t" .. labels.name(assets.patch_card(card.id), card.id) .. "\n") end
    end
    return table.concat(lines)
end
return {update = update, as_text = as_text}
