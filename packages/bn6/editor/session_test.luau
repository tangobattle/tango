--!strict
local session = require("@bn-common/editor/session")

local function test()
    for _, inline in {false, true} do
        for _, mode in {"unmanaged", "viewer", "editable", "editing", "saving"} do
            for _, play in {"absent", "disabled", "enabled"} do
                local edit_context: EditContext? = if mode == "unmanaged" then nil else {
                    editable = mode ~= "viewer", editing = mode == "editing" or mode == "saving",
                    saving = mode == "saving", can_save = mode == "editing",
                }
                local actions: {[string]: HostAction} = {}
                if play ~= "absent" then actions.play = {enabled = play == "enabled", while_editing = false} end
                local context: EditorContext = {read_only = not edit_context or not edit_context.editing,
                    session = edit_context, embedding = {inline_actions = inline, streamer_mode = false, actions = actions}}
                local node = session.controls(context)
                assert(node.kind == "row")
                local expected: {string} = {}
                if inline and edit_context then
                    if edit_context.editing then expected = {"editor.session:cancel", "editor.session:save"}
                    elseif edit_context.editable then expected = {"editor.session:begin"} end
                end
                if (not edit_context or not edit_context.editing) and play ~= "absent" then
                    table.insert(expected, "editor.host:play")
                end
                assert(#node.children == #expected)
                for index, child in node.children do
                    assert(child.kind == "button" and child.id == expected[index])
                    if child.id == "editor.host:play" then assert(child.enabled == (play == "enabled")) end
                end
            end
        end
    end
    local effects = session.update({kind = "activate", id = "editor.host:play", value = ""})
    assert(effects and #effects == 1)
    local effect = effects[1]
    assert(effect.kind == "invoke" and effect.id == "play")
end

return {test = test}
