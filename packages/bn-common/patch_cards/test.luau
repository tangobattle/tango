--!strict
local catalog = require("./catalog")
local edit = require("./edit")
local rules = require("./rules")
local state = require("./state")
local presentation = require("./ui")

local function test(t: Translate)
    local cards: {edit.Card} = {{id = 1, enabled = true}, {id = 2, enabled = false}}
    local count, rebuilt = 2, 0
    local view: edit.View = {
        count = function(): number return count end,
        card = function(slot: number): edit.Card? return if slot >= 1 and slot <= count then cards[slot] else nil end,
        set_count = function(n: number): boolean count = n; return true end,
        set_card = function(slot: number, card: edit.Card): boolean
            if slot < 1 or slot > count then return false end
            cards[slot] = card
            return true
        end,
        rebuild_anticheat = function() rebuilt += 1 end,
    }
    local info: {[number]: catalog.Info} = {
        [1] = {name = "Éclair", mb = 30, effects = {}},
        [2] = {name = "Alpha", mb = 70, effects = {}},
        [3] = {name = "Zed", mb = 60, effects = {}},
        [4] = {mb = 0, effects = {}},
    }
    local assets: catalog.Assets = {num_cards = 6, card = function(id: number): catalog.Info? return info[id] end}
    local usage = rules.scan(view, assets)
    assert(usage.memory == 30 and usage.registered[2] and next(usage.overflow) == nil)
    assert(edit.apply(view, assets.num_cards, {kind = "add", id = 3}))
    usage = rules.scan(view, assets)
    assert(count == 3 and cards[3].enabled and usage.memory == 90 and usage.overflow[3])
    assert(not edit.apply(view, assets.num_cards, {kind = "add", id = 3}) and rebuilt == 1)
    assert(edit.apply(view, assets.num_cards, {kind = "move", from = 3, to = 1}))
    usage = rules.scan(view, assets)
    assert(cards[1].id == 3 and cards[2].id == 1 and not cards[3].enabled)
    assert(usage.overflow[2] and not usage.overflow[1] and not usage.overflow[3])
    assert(not edit.apply(view, assets.num_cards, {kind = "remove", slot = 4}) and rebuilt == 2)
    assert(edit.apply(view, assets.num_cards, {kind = "remove", slot = 1}))
    assert(count == 2 and cards[1].id == 1 and cards[2].id == 2 and not cards[2].enabled)
    assert(edit.apply(view, assets.num_cards, {kind = "clear"}))
    assert(count == 0 and cards[1].id == 1 and cards[2].id == 2 and rebuilt == 4)

    local scratch: ViewState = {}
    assert(state.update(scratch, view, 6, {kind = "change", id = "patch_cards.filter", value = "ÉC"}))
    assert(state.read(scratch).filter == "ÉC")
    local entries = catalog.entries(assets, "id", state.read(scratch).filter, {})
    assert(#entries == 1 and entries[1].id == 1)
    entries = catalog.entries(assets, "mb", "", {[1] = true, [2] = true})
    assert(#entries == 2 and entries[1].id == 4 and entries[2].id == 3)
    assert(state.update(scratch, view, 6, {kind = "change", id = "patch_cards.sort", value = "name"}))
    assert(state.read(scratch).sort == "name")
    assert(state.update(scratch, view, 6, {kind = "activate", id = "patch_cards.add:4", value = ""}))
    assert(count == 1 and cards[1].id == 4 and cards[1].enabled)
    assert(not state.update(scratch, view, 6, {kind = "activate", id = "unrelated", value = ""}))
    assert(presentation.view(view, assets, scratch, t).kind == "row")
end

return {test = test}
