--!strict
local panel = require("@editor-common/cover")
local save = require("../save")
local t = require("../i18n")
local motion = require("@editor-common/navigation_motion")

local function visible(state: ViewState, context: EditorContext): boolean
    return context.embedding.streamer_mode and state["editor.reviewing"] ~= "true"
end

local function view(bytes: buffer): Node
    local prefix = if save.info(bytes).region == "JP" then "exe6" else "bn6"
    local logos = {
        tango.decode_png(tango.read_file("editor/cover/logos/" .. prefix .. "-0.png")),
        tango.decode_png(tango.read_file("editor/cover/logos/" .. prefix .. "-1.png")),
    }
    return panel.view(logos, "editor.review", t("save-review"))
end

local function update(state: ViewState, action: Action): boolean
    if action.kind ~= "activate" or action.id ~= "editor.review" then return false end
    state["editor.reviewing"] = "true"
    motion.restart(state, 0)
    return true
end

return {visible = visible, view = view, update = update}
