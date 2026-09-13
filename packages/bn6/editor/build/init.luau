--!strict
local folder_rules = require("@bn-common/folder/rules")
local report = require("@bn-common/build")
local patch_cards = require("../patch_cards")
local common = require("@editor-common")
local save = require("../save")
local model = require("../save/model")
local rom = require("../rom")
local navi_stats = require("../navi/stats")
local t = require("../i18n")

export type PatchFinding = report.PatchFinding
export type PartFinding = report.PartFinding
export type Report = report.Report

local function limits(value: model.Model, assets: rom.Assets): folder_rules.Limits
    local stats = navi_stats.derive(value, assets)
    return {mega_limit = stats.mega_limit, giga_limit = stats.giga_limit,
        reg_memory = stats.reg_memory, tag_memory = stats.tag_memory,
        max_copies = function(chip: folder_rules.ChipInfo): number return navi_stats.max_copies(chip) end}
end

local function folder_view(value: model.Model, folder: number): folder_rules.View
    return {size = 30, chip = function(slot: number): folder_rules.Chip? return value.chip(folder, slot) end}
end

local function folder_assets(assets: rom.Assets): folder_rules.Assets
    return {chip = function(id: number): folder_rules.ChipInfo?
        local chip = assets.chip(id)
        return if chip then {name = chip.name, codes = chip.codes, class = chip.class, dark = false, mb = chip.mb} else nil
    end, chip_is_legal = assets.chip_is_legal}
end

local function report_assets(assets: rom.Assets): report.Assets
    return {folder = folder_assets(assets), patch_cards = patch_cards.assets(assets), navicust_part = assets.navicust_part}
end

local function inspect(bytes: buffer, assets: rom.Assets): Report
    local value = model.open(bytes)
    local folder = value.equipped_folder()
    local view: report.View = {folder = folder_view(value, folder),
        selections = {regular = value.regular_index(folder), tags = value.tag_indexes(folder)},
        patch_cards = if value.patch_count() ~= nil then patch_cards.view(value) else nil,
    }
    local actual = value.materialized()
    if actual then
        local expected = model.open(common.copy(bytes))
        assert(expected.rebuild_materialized(assets))
        view.navicust = {count = 25, actual = actual, expected = expected.materialized() :: {number}, part = value.navicust_part}
    end
    return report.inspect(view, report_assets(assets), limits(value, assets))
end

local function warnings(value: Report, assets: rom.Assets): {string}
    return report.warnings(value, report_assets(assets), t)
end

local function validate(bytes: buffer): {string}
    if save.folders(bytes) < 1 or save.folders(bytes) > 3 then return {t("invalid-folder-count")} end
    local wram = buffer.create(save.SIZE)
    buffer.copy(wram, 0, bytes, save.START, save.SIZE)
    local assets = rom.from_inputs(wram)
    if not assets then return save.validate(bytes) end
    local info = save.info(bytes)
    if assets.definition.region ~= info.region or assets.definition.variant ~= info.variant then
        return tango.fail(t, "rom-save-mismatch")
    end
    return warnings(inspect(bytes, assets), assets)
end

local function tab_warnings(value: Report, assets: rom.Assets, tab: string): {string}
    return report.tab_warnings(value, report_assets(assets), tab, t)
end

return {limits = limits, folder_view = folder_view, folder_assets = folder_assets,
    inspect = inspect, warnings = warnings, tab_warnings = tab_warnings, validate = validate}
