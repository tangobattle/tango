--!strict
local edit = require("@bn-common/folder/edit")
local rules = require("@bn-common/folder/rules")
local model = require("../save/model")
local rom = require("../rom")
local build = require("../build")
export type Action = edit.Action

local function view(value: model.Model, folder: number): edit.View
    return {
        size = 30,
        chip = function(slot: number): rules.Chip? return value.chip(folder, slot) end,
        regular = function(): number? return value.regular_index(folder) end,
        tags = function(): {number}? return nil end,
        set_chip = function(slot: number, chip: rules.Chip?): boolean return value.set_chip(folder, slot, chip) end,
        set_regular = function(slot: number?): boolean return value.set_regular_index(folder, slot) end,
        set_tags = function(_pair: {number}?): boolean return false end,
        rebuild_anticheat = value.rebuild_chip_anticheat,
    }
end

return {view = view, apply = function(value: model.Model, assets: rom.Assets, folder: number, action: Action)
    edit.apply(view(value, folder), build.folder_assets(assets), build.limits(value, assets), action)
end}
