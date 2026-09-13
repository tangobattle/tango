--!strict
local model = require("./save/model")
local rom = require("./rom")
local catalog_assets = require("./folder/catalog")
local catalog = require("@bn-common/auto_battle_data/catalog")
local controller = require("@bn-common/auto_battle_data/state")
local ui = require("@bn-common/auto_battle_data/ui")
local t = require("./i18n")
local function view(value: model.Model, assets: rom.Assets): controller.View
    return {
        use_count = function(id: number, secondary: boolean): number return value.chip_use_count(id, secondary) or 0 end,
        set_use_count = value.set_chip_use_count,
        rebuild = function() value.rebuild_auto_battle_data(catalog.ranking_assets(catalog_assets(assets))) end,
    }
end
return {
    view = view,
    editor = function(value: model.Model, assets: rom.Assets, state: ViewState): Node
        return ui.view(view(value, assets), catalog_assets(assets), state, t)
    end,
    viewer = function(value: model.Model, assets: rom.Assets): Node
        return ui.viewer(view(value, assets), catalog_assets(assets), t)
    end,
    update = function(value: model.Model, assets: rom.Assets, state: ViewState, action: Action)
        if not controller.update(view(value, assets), 368, state, action) then
            tango.fail(t, "unknown-action")
        end
    end,
}
