--!strict
local folder_rules = require("../folder/rules")
local card_rules = require("../patch_cards/rules")
local card_edit = require("../patch_cards/edit")
local card_catalog = require("../patch_cards/catalog")
export type PatchFinding = {slot: number, id: number, mb: number, used: number, limit: number}
export type PartFinding = {slot: number, id: number, col: number, row: number}
export type Report = {
    folder: {folder_rules.Finding}, patch_cards: {PatchFinding}, navicust_parts: {PartFinding},
    navicust_mismatch: boolean, patch_mb: number?,
}

export type Assets = {
    folder: folder_rules.Assets, patch_cards: card_catalog.Assets,
    navicust_part: (number) -> {read name: string?}?,
}
export type View = {
    folder: folder_rules.View, selections: folder_rules.Selections, patch_cards: card_edit.View?,
    navicust: {count: number, actual: {number}, expected: {number},
        part: (number) -> {read id: number, read col: number, read row: number}?}?,
}
local function inspect(view: View, assets: Assets, limits: folder_rules.Limits): Report
    local result: Report = {
        folder = folder_rules.violations(view.folder, assets.folder, limits, view.selections),
        patch_cards = {}, navicust_parts = {}, navicust_mismatch = false,
    }
    local cards = view.patch_cards
    if cards then
        local usage = card_rules.scan(cards, assets.patch_cards)
        result.patch_mb = usage.memory
        for slot = 1, cards.count() do
            if not usage.overflow[slot] then continue end
            local card = cards.card(slot)
            if not card then continue end
            local info = assets.patch_cards.card(card.id)
            table.insert(result.patch_cards, {slot = slot, id = card.id, mb = if info then info.mb else 0,
                used = usage.memory, limit = card_rules.MAX_MEMORY})
        end
    end
    local navicust = view.navicust
    if navicust then
        local slots: {[number]: boolean} = {}
        for index, cell in navicust.actual do
            if cell == navicust.expected[index] then continue end
            result.navicust_mismatch = true
            for _, slot in {cell, navicust.expected[index]} do
                if slot > 0 and navicust.part(slot) then slots[slot] = true end
            end
        end
        for slot = 1, navicust.count do
            local part = navicust.part(slot)
            if slots[slot] and part then
                table.insert(result.navicust_parts, {slot = slot, id = part.id, col = part.col, row = part.row})
            end
        end
    end
    return result
end

local function warnings(report: Report, assets: Assets, t: Translate): {string}
    local result = folder_rules.warnings(report.folder, assets.folder, t)
    if #report.patch_cards > 0 then
        local cards = table.clone(report.patch_cards)
        table.sort(cards, function(a: PatchFinding, b: PatchFinding): boolean return if a.id ~= b.id then a.id < b.id else a.slot < b.slot end)
        local labels: {string} = {}
        local seen: {[number]: boolean} = {}
        for _, finding in cards do
            if seen[finding.id] then continue end
            seen[finding.id] = true
            local info = assets.patch_cards.card(finding.id)
            local name = if info and info.name then info.name else t("build-patch-card-unknown", {id = finding.id})
            table.insert(labels, name .. " (" .. tostring(finding.mb) .. "MB)")
        end
        table.insert(result, t("build-violation", {subject = table.concat(labels, ", "),
            reason = t("build-violation-patch-cards-exceed-memory", {used = report.patch_mb :: number, limit = card_rules.MAX_MEMORY})}))
    end
    if #report.navicust_parts > 0 then
        local parts = table.clone(report.navicust_parts)
        table.sort(parts, function(a: PartFinding, b: PartFinding): boolean
            if a.id ~= b.id then return a.id < b.id end
            if a.row ~= b.row then return a.row < b.row end
            if a.col ~= b.col then return a.col < b.col end
            return a.slot < b.slot
        end)
        local labels: {string} = {}
        local seen: {[string]: boolean} = {}
        for _, finding in parts do
            local key = tostring(finding.id) .. ":" .. tostring(finding.row) .. ":" .. tostring(finding.col)
            if seen[key] then continue end
            seen[key] = true
            local info = assets.navicust_part(finding.id)
            local name = if info and info.name then info.name else t("build-navicust-part-unknown", {id = finding.id})
            table.insert(labels, name)
        end
        table.insert(result, t("build-violation", {subject = table.concat(labels, ", "),
            reason = t("build-violation-navicust-invalid-shape-reason")}))
    elseif report.navicust_mismatch then
        table.insert(result, t("build-violation-navicust-materialization"))
    end
    return result
end

local function tab_warnings(report: Report, assets: Assets, tab: string, t: Translate): {string}
    local scoped: Report = {
        folder = if tab == "folder" then report.folder else {},
        patch_cards = if tab == "patch_cards" then report.patch_cards else {},
        navicust_parts = if tab == "navicust" then report.navicust_parts else {},
        navicust_mismatch = tab == "navicust" and report.navicust_mismatch,
        patch_mb = report.patch_mb,
    }
    return warnings(scoped, assets, t)
end

return {inspect = inspect, warnings = warnings, tab_warnings = tab_warnings}
