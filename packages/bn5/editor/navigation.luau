--!strict
local tabs = require("@editor-common/tabs")
local model = require("./save/model")
local rom = require("./rom")
local build = require("./build")
local folder = require("./folder/ui")
local clipboard = require("./clipboard")
local motion = require("@editor-common/navigation_motion")
local t = require("./i18n")

local function available(value: model.Model): {string}
    local result: {string} = {}
    if value.has_navicust() then table.insert(result, "navicust") end
    table.insert(result, "folder")
    if value.patch_count() ~= nil then table.insert(result, "patch_cards") end
    table.insert(result, "auto_battle_data")
    return result
end

local function selected(value: model.Model, state: ViewState): string
    local choices = available(value)
    local tab = state["editor.tab"]
    return if tab and table.find(choices, tab) then tab else choices[1]
end

local function view(bytes: buffer, value: model.Model, assets: rom.Assets, state: ViewState, read_only: boolean, inline_actions: boolean?): Node
    local report = build.inspect(bytes, assets)
    local buttons: {tabs.Tab} = {}
    for _, tab in available(value) do
        table.insert(buttons, {id = "editor.tab:" .. tab,
            label = t(if tab == "folder" then "save-tab-folder" else if tab == "navicust" then "save-tab-navicust" else if tab == "patch_cards" then "save-tab-patch-cards" else "save-tab-auto-battle-data"),
            icon = if tab == "folder" then "\u{e0cf}" else if tab == "navicust" then "\u{e29c}" else if tab == "patch_cards" then "\u{e0aa}" else "\u{e1bb}",
            errors = build.tab_warnings(report, assets, tab),
        })
    end
    local active = selected(value, state)
    local extras: {Node} = {}
    if read_only and inline_actions ~= false then
        local previous = state["editor.prev_tab"]
        if active == "folder" then
            table.insert(extras, motion.tail(folder.group_control(state), state, "group", previous == "folder"))
        end
        for _, button in clipboard.buttons(active) do
            local image = button.kind == "button" and string.find(button.id, "^editor%.copy_image:") ~= nil
            local carried = if image then previous == "navicust" else previous ~= nil
            table.insert(extras, motion.tail(button, state, if image then "image" else "copy", carried))
        end
    end
    local tail: Node = {kind = "row", spacing = 6, align_y = "center" :: Align, children = extras}
    return tabs.view(buttons, "editor.tab:" .. active, tail, "editor.tabs", tonumber(state["editor.tab_scroll"] or "0") or 0)
end

local function update(value: model.Model, state: ViewState, action: Action): {Effect}?
    if action.kind == "scroll" and action.id == "editor.tabs" then
        state["editor.tab_scroll"] = tostring(action.x)
        return {}
    end
    if action.kind ~= "activate" then return nil end
    local choices = available(value)
    for index, tab in choices do
        if action.id ~= "editor.tab:" .. tab then continue end
        if state["editor.tab"] ~= tab then
            local previous = state["editor.tab"] and table.find(choices, state["editor.tab"] :: string)
            motion.restart(state, if previous and index < previous then -24 else 24)
            state["editor.prev_tab"] = state["editor.tab"]
        end
        state["editor.tab"], state["editor.tab_scroll"] = tab, "0"
        state["navi.picker"] = nil
        state["navicust.hover.col"], state["navicust.hover.row"] = nil, nil
        state["navicust.viewer.col"], state["navicust.viewer.row"] = nil, nil
        return {
            {kind = "scroll_to", id = "editor.body", x = 0, y = 0} :: Effect,
            {kind = "scroll_to", id = "editor.tabs", x = 0, y = 0} :: Effect,
        }
    end
    return nil
end

return {available = available, selected = selected, view = view, update = update}
