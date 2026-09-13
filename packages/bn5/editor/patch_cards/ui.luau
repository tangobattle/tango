--!strict
local t = require("../i18n")
local presentation = require("@bn-common/patch_cards/ui")
local read_only = require("@bn-common/patch_cards/ui/viewer")
local controller = require("@bn-common/patch_cards/state")
local cards = require("./init")
local model = require("../save/model")
local rom = require("../rom")

local function view(value: model.Model, assets: rom.Assets, state: ViewState): Node
    return presentation.view(cards.view(value), cards.assets(assets), state, t)
end

local function update(value: model.Model, _assets: rom.Assets, state: ViewState, action: Action)
    if not controller.update(state, cards.view(value), 112, action) then
        tango.fail(t, "unknown-action")
    end
end

local function viewer(value: model.Model, assets: rom.Assets): Node
    return read_only.view(cards.view(value), cards.assets(assets), t)
end

return {view = view, update = update, viewer = viewer}
