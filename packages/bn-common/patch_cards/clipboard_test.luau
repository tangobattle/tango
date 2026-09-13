--!strict
local clipboard = require("./clipboard")
local catalog = require("./catalog")
local edit = require("./edit")

local function test()
    local cards: {edit.Card} = {{id = 2, enabled = true}, {id = 1, enabled = false}, {id = 9, enabled = true}, {id = 3, enabled = true}}
    local view: edit.View = {count = function(): number return #cards end,
        card = function(slot: number): edit.Card? return cards[slot] end,
        set_count = function(_count: number): boolean return false end,
        set_card = function(_slot: number, _card: edit.Card): boolean return false end,
        rebuild_anticheat = function() end}
    local assets: catalog.Assets = {num_cards = 4, card = function(id: number): catalog.Info?
        if id == 9 then return nil end
        return {name = if id == 2 then "保存" else nil, mb = id * 10, effects = {}}
    end}
    assert(clipboard.text(view, assets) == "保存\t20MB\n#9\t0MB\n#3\t30MB\n")
    for _, card in cards do card.enabled = false end
    assert(clipboard.text(view, assets) == "")
    table.clear(cards)
    assert(clipboard.text(view, assets) == "")
end

return {test = test}
