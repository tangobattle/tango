--!strict
local catalog = require("./catalog")
local state = require("./state")
local edit = require("./edit")
local rules = require("./rules")

return {test = function()
    local function chip(name: string?, codes: {number}, attack: number, element: number, mb: number, class: string): catalog.Chip
        return {name = name, codes = codes, attack = attack, element = element, mb = mb, class = class,
            icon = function(): Raster? return nil end, image = function(): Raster? return nil end}
    end
    local chips: {[number]: catalog.Chip} = {
        [1] = chip("Éclair", {0, 26, 27}, 10, 2, 4, "standard"),
        [2] = chip("alpha", {1}, 40, 0, 6, "mega"),
        [3] = chip("alpha", {2}, 5, 1, 2, "standard"),
        [4] = chip(nil, {0}, 1, 0, 1, "standard"),
        [5] = chip("Illegal", {0}, 1, 0, 1, "standard"),
    }
    local assets: catalog.Assets = {num_chips = 6, chips_have_mb = true,
        chip = function(id: number): catalog.Chip? return chips[id] end,
        chip_is_legal = function(id: number): boolean return id > 0 and id < 5 end,
        element_icon = function(_id: number): Raster? return nil end}
    local function order(sort: catalog.Sort, query: string): string
        local result: {string} = {}
        for _, entry in catalog.entries(assets, sort, query) do table.insert(result, entry.id .. ":" .. entry.code) end
        return table.concat(result, ",")
    end
    assert(order("id", "") == "1:0,1:26,2:1,3:2")
    assert(order("name", "") == "2:1,3:2,1:0,1:26")
    assert(order("code", "") == "1:0,2:1,3:2,1:26")
    assert(order("attack", "") == "3:2,1:0,1:26,2:1")
    assert(order("element", "") == "2:1,3:2,1:0,1:26")
    assert(order("mb", "") == "3:2,1:0,1:26,2:1")
    assert(order("id", "ÉC") == "1:0,1:26")
    assert(order("id", "ALP") == "2:1,3:2")
    assert(order("id", ".") == "")

    local slots: {[number]: rules.Chip?} = {}
    local regular: number? = nil
    local pair: {number}? = nil
    local writes = 0
    local view: edit.View = {
        size = 4, chip = function(slot: number): rules.Chip? return slots[slot] end,
        regular = function(): number? return regular end, tags = function(): {number}? return pair end,
        set_chip = function(slot: number, value: rules.Chip?): boolean slots[slot] = value; return true end,
        set_regular = function(slot: number?): boolean regular = slot; return true end,
        set_tags = function(value: {number}?): boolean pair = value; return true end,
        rebuild_anticheat = function() writes += 1 end,
    }
    local limits: rules.Limits = {mega_limit = 1, reg_memory = 5, tag_memory = 10,
        max_copies = function(_chip: rules.ChipInfo): number return 5 end}
    local scratch: ViewState = {}
    local function tags(): string? return scratch["folder.tags"] end
    local function apply(id: string, value: string?)
        assert(state.update(scratch, view, catalog.rule_assets(assets), limits, {kind = "activate", id = id, value = value or ""}))
    end
    apply("folder.add:1:0", "1:0")
    apply("folder.add:2:1", "2:1")
    apply("folder.tag:1")
    assert(tags() == "1" and pair == nil)
    apply("folder.add:1:26", "1:26")
    assert(tags() == "2" and pair == nil)
    apply("folder.tag:3")
    assert(tags() == "2,3" and pair and pair[1] == 2 and pair[2] == 3)
    apply("folder.remove:2")
    assert(tags() == "2" and pair == nil)
    local before = writes
    apply("folder.regular:2")
    assert(writes == before and regular == nil)
    assert(state.update(scratch, view, catalog.rule_assets(assets), limits, {kind = "reorder", id = "folder.slots", from = 2, to = 1}))
    assert(tags() == "1")
    assert(state.update(scratch, view, catalog.rule_assets(assets), limits, {kind = "reorder", id = "folder.slots", from = 1, to = 4}))
    assert(tags() == "2")
    apply("folder.clear")
    assert(tags() == "" and pair == nil and slots[1] == nil)
    apply("folder.add:2:1", "2:1")
    before = writes
    apply("folder.add:2:1", "2:1")
    apply("folder.regular:1")
    assert(writes == before and regular == nil)
    assert(state.update(scratch, view, catalog.rule_assets(assets), limits, {kind = "change", id = "folder.filter", value = "ÉCLAIR"}))
    assert(state.update(scratch, view, catalog.rule_assets(assets), limits, {kind = "change", id = "folder.sort", value = "attack"}))
    local current = state.read(scratch, view)
    assert(current.filter == "ÉCLAIR" and current.sort == "attack")
    pair = {1, 2}
    assert(#state.read({}, view).tags == 2)
    assert(not state.update(scratch, view, catalog.rule_assets(assets), limits, {kind = "activate", id = "other", value = ""}))
end}
