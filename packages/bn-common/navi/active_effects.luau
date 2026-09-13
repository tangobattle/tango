--!strict
local navicust = require("../rom/navicust")
local cards = require("../rom/patch_cards")
export type PartsView = {
    read materialized: () -> {number}?,
    read navicust_part: (number) -> {read id: number}?,
}
export type CardsView = {
    read patch_count: () -> number?,
    read patch_card: (number) -> {read id: number, read enabled: boolean}?,
}
export type PartsAssets = {
    read navicust_part: (number) -> {read effects: {navicust.Effect}}?,
}
export type CardsAssets = {
    read patch_card: (number) -> {read effects: {cards.Effect}}?,
}
export type View = PartsView & CardsView
export type Assets = PartsAssets & CardsAssets

local function parts(view: PartsView, assets: PartsAssets, width: number, row: number?): {navicust.Effect}
    local result: {navicust.Effect} = {}
    local grid = view.materialized()
    if not grid then return result end
    local seen: {[number]: boolean} = {}
    local first = if row then row * width + 1 else 1
    local last = if row then first + width - 1 else #grid
    for index = first, last do
        local slot = grid[index] or 0
        if slot == 0 or seen[slot] then continue end
        seen[slot] = true
        -- Materialized cells already contain the 1-based placement slot.
        local part = view.navicust_part(slot)
        local info = if part then assets.navicust_part(part.id) else nil
        if info then
            for _, effect in info.effects do table.insert(result, effect) end
        end
    end
    return result
end

local function patch_cards(view: CardsView, assets: CardsAssets): {cards.Effect}
    local result: {cards.Effect} = {}
    for slot = 1, view.patch_count() or 0 do
        local card = view.patch_card(slot)
        if not card or not card.enabled then continue end
        local info = assets.patch_card(card.id)
        if info then
            for _, effect in info.effects do table.insert(result, effect) end
        end
    end
    return result
end

return {parts = parts, patch_cards = patch_cards}
