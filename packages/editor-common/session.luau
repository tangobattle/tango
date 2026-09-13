--!strict
export type Labels = {edit: string, cancel: string, save: string}

local function button(id: string, icon: string, label: string, primary: boolean, enabled: boolean): Node
    return {kind = "button", id = id, enabled = enabled, content_padding = {4, 10, 4, 10},
        tone = (if primary then "primary" else "neutral") :: Tone,
        child = {kind = "row" :: "row", spacing = 8, align_y = "center" :: Align, children = {
            {kind = "text" :: "text", text = icon, font = "icons" :: Font} :: Node,
            {kind = "text" :: "text", text = label} :: Node,
        }} :: Node}
end

local function controls(context: EditContext, labels: Labels): Node
    local children: {Node} = {}
    if context.editing then
        table.insert(children, button("editor.session:cancel", "\u{e1b2}", labels.cancel, false, not context.saving))
        table.insert(children, button("editor.session:save", "\u{e06c}", labels.save, true, context.can_save))
    elseif context.editable then
        table.insert(children, button("editor.session:begin", "\u{e1f9}", labels.edit, false, true))
    end
    return {kind = "row", spacing = 6, align_y = "center" :: Align, children = children}
end

local function update(action: Action): {Effect}?
    if action.kind ~= "activate" then return nil end
    local host_id = string.match(action.id, "^editor%.host:(.+)$")
    if host_id then return {{kind = "invoke", id = host_id} :: Effect} end
    local commands: {[string]: EditCommand} = {
        ["editor.session:begin"] = "begin", ["editor.session:cancel"] = "cancel", ["editor.session:save"] = "save",
    }
    local command = commands[action.id]
    return if command then {{kind = "edit" :: "edit", command = command :: EditCommand} :: Effect} else nil
end

local function host_action(id: string, icon: string, label: string, enabled: boolean): Node
    local node = button("editor.host:" .. id, icon, label, enabled, enabled)
    assert(node.kind == "button" and node.child and node.child.kind == "row")
    node.child.spacing = 6
    return node
end

return {controls = controls, update = update, host_action = host_action}
