--!strict
local controls = require("@editor-common/clipboard")
local folder_copy = require("@bn-common/folder/clipboard")
local navicust_copy = require("@bn-common/navicust/clipboard")
local patch_card_copy = require("@bn-common/patch_cards/clipboard")
local folder = require("./folder")
local folder_assets = require("./folder/catalog")
local navicust = require("./navicust")
local cards = require("./patch_cards")
local model = require("./save/model")
local rom = require("./rom")
local abd = require("./auto_battle_data")
local abd_ui = require("@bn-common/auto_battle_data/ui")
local t = require("./i18n")

local function buttons(tab: string): {Node}
    local result: {Node} = {}
    if tab == "navicust" then
        table.insert(result, controls.button("editor.copy_image:navicust", "\u{e53c}", t("save-copy-image"), t("copied")))
    end
    table.insert(result, controls.button("editor.copy:" .. tab, "\u{e225}", t("save-copy"), t("copied")))
    return result
end

local function update(value: model.Model, assets: rom.Assets, state: ViewState, action: Action): {Effect}?
    if action.kind ~= "activate" then return nil end
    if action.id == "editor.copy:folder" then
        return {folder_copy.copy(folder.view(value, value.equipped_folder()), folder_assets(assets), state["folder.grouped"] ~= "false")}
    elseif action.id == "editor.copy:auto_battle_data" then
        return {{kind = "copy_text", text = abd_ui.text(abd.view(value, assets), folder_assets(assets), t)} :: Effect}
    elseif action.id == "editor.copy:patch_cards" then
        return {{kind = "copy_text", text = patch_card_copy.text(cards.view(value), cards.assets(assets))} :: Effect}
    elseif action.id == "editor.copy:navicust" then
        return {{kind = "copy_text", text = navicust_copy.text(navicust.view(value, assets), navicust.assets(assets), nil)} :: Effect}
    elseif action.id == "editor.copy_image:navicust" then
        return {{kind = "copy_image", image = navicust_copy.image(navicust.view(value, assets), navicust.assets(assets),
            value.materialized() :: {number})} :: Effect}
    end
    return nil
end

return {buttons = buttons, update = update}
