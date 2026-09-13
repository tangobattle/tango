--!strict
local catalog = require("./catalog")
local rules = require("./rules")
local t = require("../i18n")

local function name(card: catalog.Card?, id: number): string
    return if card and card.name ~= "" then card.name else "#" .. tostring(id)
end
local function slot(index: number): string
    return ({"0A", "0B", "0C", "0D", "0E", "0F"})[index] or "#" .. tostring(index)
end
local function effect(value: catalog.Effect): string
    local kind = value.kind
    if value.kind == "max_hp" or value.kind == "custom_slots" or value.kind == "mega_folder" or value.kind == "giga_folder" then
        return t("mod-card-effect-" .. kind, {value = value.value})
    elseif value.kind == "buster_attack" then return t("mod-card-effect-buster_attack", {value = value.value + 1})
    elseif value.kind == "b_button" or value.kind == "b_charge" or value.kind == "b_left" then
        return t("mod-card-effect-" .. kind, {shot = t("mod-card-shot-" .. value.shot)})
    elseif value.kind == "panel_step" then return t("mod-card-effect-panel_step", {panel = t("mod-card-panel-" .. value.panel)})
    elseif value.kind == "aura" then return t("mod-card-aura-" .. value.aura)
    elseif value.kind == "soul" then return t("mod-card-effect-soul", {soul = t("mod-card-soul-" .. value.soul)})
    elseif value.kind == "color" or value.kind == "pet_menu" then
        return t("mod-card-effect-" .. kind, {color = t("mod-card-color-" .. value.color)})
    else return t("mod-card-effect-" .. kind) end
end
local function bugs(values: {catalog.Bug}): string?
    if #values == 0 then return nil end
    local parts: {string} = {}
    for _, bug in values do table.insert(parts, t("mod-card-bug-" .. bug.kind)) end
    return table.concat(parts, t("mod-card-bug-separator"))
end
local function reason(finding: rules.Finding): string
    return if finding.kind == "wrong_slot" then t("build-violation-patch-card4-wrong-slot-reason",
        {actual_slot = slot(finding.slot), expected_slot = slot(finding.expected_slot :: number)})
        else t("build-violation-patch-card4-not-in-catalog-reason", {actual_slot = slot(finding.slot)})
end
local function warnings(findings: {rules.Finding}, assets: {read patch_card: (number) -> catalog.Card?}): {string}
    local result: {string} = {}
    for _, finding in findings do
        table.insert(result, t("build-violation", {subject = name(assets.patch_card(finding.id), finding.id), reason = reason(finding)}))
    end
    return result
end
return {name = name, slot = slot, effect = effect, bugs = bugs, reason = reason, warnings = warnings}
