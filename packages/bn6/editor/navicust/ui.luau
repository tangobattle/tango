--!strict
local ui = require("@bn-common/navicust/ui")
local read_only = require("@bn-common/navicust/ui/viewer")
local shape = require("@bn-common/navicust/shape")
local adapter = require("./init")
local model = require("../save/model")
local rom = require("../rom")
local t = require("../i18n")

local function view(value: model.Model, source: rom.Assets, state: ViewState, read_only_mode: boolean?): Node
    local parts, assets = adapter.view(value, source), adapter.assets(source)
    local expected, actual = shape.materialize(parts, assets), value.materialized() :: {number}
    local illegal: {[number]: boolean} = {}
    for index, cell in actual do
        if cell == expected[index] then continue end
        for _, slot in {cell, expected[index]} do if slot > 0 then illegal[slot] = true end end
    end
    return if read_only_mode then read_only.view(parts, assets, actual, illegal, state, t)
        else ui.view(parts, assets, state, illegal, t)
end

local function update(value: model.Model, source: rom.Assets, state: ViewState, action: Action)
    ui.update(state, adapter.view(value, source), adapter.assets(source), action)
end

local function viewer_update(source: rom.Assets, state: ViewState, action: Action): boolean
    return read_only.update(adapter.assets(source).layout, state, action)
end

local function update_state(state: ViewState, action: Action, read_only_mode: boolean): boolean
    return if read_only_mode then read_only.update(adapter.size, state, action)
        else ui.update_state(adapter.size, state, action)
end

return {view = view, update = update, viewer_update = viewer_update, update_state = update_state}
