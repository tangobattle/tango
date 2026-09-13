--!strict
local model = require("./model")
local catalog = require("./catalog")
local controller = require("./state")
local ui = require("./ui")
local folder = require("../folder/catalog")

return function(t: Translate)
    local primary: {[number]: number} = {[0] = 5, [1] = 7, [2] = 8, [3] = 9, [5] = 5}
    local secondary: {[number]: number} = {[5] = 10}
    local rebuilt = 0
    local materialized: {model.Slot} = {}
    local assets: folder.Assets = {
        num_chips = 7, chips_have_mb = true,
        chip_is_legal = function(id: number): boolean return id ~= 5 end,
        element_icon = function(_id: number): Raster? return nil end,
        chip = function(id: number): folder.Chip?
            if id < 0 or id >= 7 then return nil end
            return {name = if id == 6 then " " elseif id == 0 then "Écho" else "Chip " .. tostring(id),
                codes = {0}, class = ({"standard", "mega", "giga", "program_advance", "none"})[id % 5 + 1],
                mb = 10, dark = false, element = 0, attack = 0,
                icon = function(): Raster? return nil end, image = function(): Raster? return nil end}
        end,
    }
    local counts: model.View = {use_count = function(id: number, second: boolean): number
        return (if second then secondary else primary)[id] or 0
    end}
    local view: controller.View = {
        use_count = counts.use_count,
        set_use_count = function(id: number, count: number, second: boolean): boolean
            if id < 0 or id >= 7 then return false end
            (if second then secondary else primary)[id] = count
            return true
        end,
        rebuild = function()
            rebuilt += 1
            materialized = model.materialize(counts, catalog.ranking_assets(assets))
        end,
    }
    view.rebuild()
    -- ID zero is a real chip. Ranking includes used chips outside the editable
    -- legal library, and ties retain ID order; empty runs remain dense records.
    assert(#materialized == 42 and materialized[1].id == 5)
    assert(materialized[4].id == 0 and materialized[7].id == 0 and materialized[8].id == 5)
    for slot = 34, 41 do assert(materialized[slot].id == nil) end
    local entries = catalog.entries(view, assets, "used", "")
    assert(#entries == 4 and entries[1].id == 3 and entries[4].id == 0)
    entries = catalog.entries(view, assets, "name", "é")
    assert(#entries == 1 and entries[1].id == 0)
    local state: ViewState = {}
    assert(controller.update(view, 7, state, {kind = "change", id = "auto_battle_data.filter", value = "é"}))
    assert(controller.update(view, 7, state, {kind = "change", id = "auto_battle_data.sort", value = "name"}))
    assert(rebuilt == 1 and controller.read(state).filter == "é")
    assert(ui.view(view, assets, state, t).kind == "row" and ui.viewer(view, assets, t).kind == "column")
    local copied = ui.text(view, assets, t)
    assert(string.find(copied, "[" .. t("auto-battle-data-standard-chips") .. "]", 1, true) ~= nil)
    assert(string.find(copied, "4\tÉcho\n", 1, true) and string.find(copied, "1\t---\n", 1, true))
    assert(controller.update(view, 7, state, {kind = "change", id = "auto_battle_data.used:0", value = "99x9999"}))
    assert(primary[0] == 65535 and rebuilt == 2)
    assert(controller.update(view, 7, state, {kind = "change", id = "auto_battle_data.secondary:0", value = ""}))
    assert(secondary[0] == 0 and rebuilt == 3)
    assert(not controller.update(view, 7, state, {kind = "change", id = "auto_battle_data.used:7", value = "1"}))
    assert(not controller.update(view, 7, state, {kind = "change", id = "auto_battle_data.sort", value = "bad"}))
    assert(rebuilt == 3 and state["auto_battle_data.sort"] == "name")
    assert(controller.update(view, 7, state, {kind = "activate", id = "auto_battle_data.clear", value = ""}))
    for id = 0, 6 do assert(primary[id] == 0 and secondary[id] == 0) end
    for _, slot in materialized do assert(slot.id == nil) end
    assert(#materialized == 42 and rebuilt == 4)
end
