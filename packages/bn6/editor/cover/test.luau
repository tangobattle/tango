--!strict
local cover = require("./init")
local session = require("@bn-common/editor/session")

local function test()
    local state: ViewState = {["editor.tab"] = "folder"}
    local context: EditorContext = {read_only = true,
        embedding = {inline_actions = false, streamer_mode = false, actions = {}}}
    assert(not cover.visible(state, context))
    context.embedding.streamer_mode = true
    assert(cover.visible(state, context))
    assert(not cover.update(state, {kind = "change", id = "editor.review", value = "true"}))
    assert(not cover.update(state, {kind = "activate", id = "unrelated", value = ""}))
    assert(cover.visible(state, context))
    assert(cover.update(state, {kind = "activate", id = "editor.review", value = ""}))
    assert(not cover.visible(state, context) and state["editor.tab"] == "folder")
    -- The same save remains reviewed across edit-session resets and changes to
    -- streamer mode. A newly opened save starts with its own empty view state.
    session.reset_view(state)
    assert(not cover.visible(state, context))
    context.embedding.streamer_mode = false
    assert(not cover.visible(state, context))
    context.embedding.streamer_mode = true
    assert(not cover.visible(state, context) and cover.visible({}, context))
end

return {test = test}
