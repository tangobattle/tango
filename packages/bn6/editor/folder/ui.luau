--!strict
local t = require("../i18n")
local catalog_assets = require("./catalog")
local controller = require("@bn-common/folder/state")
local presentation = require("@bn-common/folder/ui")
local viewer = require("@bn-common/folder/ui/viewer")
local model = require("../save/model")
local rom = require("../rom")
local build = require("../build")
local folder = require("./init")

local function view(value: model.Model, assets: rom.Assets, state: ViewState): Node
    return presentation.view(folder.view(value, value.equipped_folder()), catalog_assets(assets),
        build.limits(value, assets), state, {regular = true, tags = true}, t)
end

local function update(value: model.Model, assets: rom.Assets, state: ViewState, action: Action)
    assert(controller.update(state, folder.view(value, value.equipped_folder()), build.folder_assets(assets),
        build.limits(value, assets), action), t("unknown-action"))
end

return {view = view, update = update,
    viewer = function(value: model.Model, assets: rom.Assets, state: ViewState): Node
        return viewer.view(folder.view(value, value.equipped_folder()), catalog_assets(assets),
            build.limits(value, assets), state["folder.grouped"] ~= "false", t)
    end,
    group_control = function(state: ViewState): Node return viewer.group_control(state["folder.grouped"] ~= "false", t) end,
}
