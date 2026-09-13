--!strict
local common = require("@editor-common/session")
local motion = require("@editor-common/navigation_motion")
local t = require("../init").t

local function controls(context: EditorContext): Node
    local children: {Node} = {}
    local editing = context.session and context.session.editing
    if context.embedding.inline_actions and context.session then
        local buttons = common.controls(context.session, {edit = t("save-edit"), cancel = t("save-edit-cancel"), save = t("save-edit-save")})
        assert(buttons.kind == "row")
        for _, button in buttons.children do table.insert(children, button) end
    end
    local play = context.embedding.actions.play
    if not editing and play then
        table.insert(children, common.host_action("play", "\u{e13c}", t("play-play"), play.enabled))
    end
    return {kind = "row", spacing = 6, align_y = "center" :: Align, children = children}
end

local function reset_view(state: ViewState)
    local keep = {"editor.tab", "editor.prev_tab", "editor.tab_scroll", "editor.reviewing", "editor.enter.revision", "folder.grouped", "folder.sort", "navicust.sort", "patch_cards.sort", "auto_battle_data.sort"}
    for key in state do if not table.find(keep, key) then state[key] = nil end end
    motion.restart(state, 0)
end

return {controls = controls, update = common.update, reset_view = reset_view}
