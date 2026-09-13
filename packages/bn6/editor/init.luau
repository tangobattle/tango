--!strict
local save = require("./save")
local model = require("./save/model")
local rom = require("./rom")
local ui = require("@editor-common/ui")
local t = require("./i18n")
local folder = require("./folder/ui")
local patch_cards = require("./patch_cards/ui")
local navicust = require("./navicust/ui")
local navi = require("./navi/ui")
local clipboard = require("./clipboard")
local session = require("@bn-common/editor/session")
local navigation = require("./navigation")
local cover = require("./cover")
local motion = require("@editor-common/navigation_motion")

local function decode(input: buffer): buffer
    local bytes = save.decode(input)
    local rom_size = tango.input_size("rom")
    if rom_size then
        local definition = rom.definition(tango.read_input("rom", 0, math.min(rom_size, 0xc0)))
        local game = save.info(bytes)
        if definition.region ~= game.region or definition.variant ~= game.variant then
            return tango.fail(t, "rom-save-mismatch")
        end
    end
    return bytes
end

local function assets_for(bytes: buffer): rom.Assets?
    local wram = buffer.create(save.SIZE)
    buffer.copy(wram, 0, bytes, save.START, save.SIZE)
    local assets = rom.from_inputs(wram)
    local game = save.info(bytes)
    if assets and (assets.definition.region ~= game.region or assets.definition.variant ~= game.variant) then
        return tango.fail(t, "rom-save-mismatch")
    end
    return assets
end

local function view(bytes: buffer, state: ViewState, context: EditorContext): Node
    if cover.visible(state, context) then return motion.body(cover.view(bytes), state) end
    local assets = assets_for(bytes)
    if not assets then return {kind = "text", text = t("rom-required")} end
    local value = model.open(bytes)
    local edit_context = context.session
    local actions: Node? = if edit_context or next(context.embedding.actions) then session.controls(context) else nil
    local selected = navigation.selected(value, state)
    local body: Node
    if not context.read_only and state["navi.picker"] == "open" then body = navi.picker(value, assets)
    else
        local content: Node
        if context.read_only then
            local viewer: Node = if selected == "patch_cards" then patch_cards.viewer(value, assets)
                else if selected == "navicust" then navicust.view(value, assets, state, true) else folder.viewer(value, assets, state)
            content = {kind = "scroll" :: "scroll", id = "editor.body", width = "fill" :: Size, child = viewer}
        else
            content = if selected == "patch_cards" then patch_cards.view(value, assets, state)
                else if selected == "navicust" then navicust.view(value, assets, state) else folder.view(value, assets, state)
        end
        local section = ui.column({navigation.view(bytes, value, assets, state, context.read_only, context.embedding.inline_actions), motion.body(content :: Node, state)}, 8)
        assert(section.kind == "column")
        section.height = (if context.read_only then "shrink" else "fill") :: Size
        body = section
    end
    local root = ui.column({navi.identity(value, assets, not context.read_only, actions), motion.region(body, state)}, 8)
    assert(root.kind == "column")
    root.height = (if context.read_only then "shrink" else "fill") :: Size
    root.enabled = not edit_context or not edit_context.saving
    return root
end

local function update(bytes: buffer, state: ViewState, action: Action, context: EditorContext): {Effect}?
    if cover.update(state, action) then return nil end
    local session_effects = session.update(action)
    if session_effects then return session_effects end
    if navicust.update_state(state, action, context.read_only) then return nil end
    local value = model.open(bytes)
    local navigation_effects = navigation.update(value, state, action)
    if navigation_effects then return navigation_effects end
    local assets = assets_for(bytes)
    if not assets then return tango.fail(t, "rom-required") end
    local effects = clipboard.update(value, assets, state, action)
    if effects then return effects end
    if action.kind == "toggle" and action.id == "folder.grouped" then
        state["folder.grouped"] = tostring(action.checked)
        return nil
    end
    if context.read_only then
        if not value.has_navicust() or not navicust.viewer_update(assets, state, action) then
            return tango.fail(t, "unknown-action")
        end
        return nil
    end
    if navi.update(value, assets, state, action) then return nil end
    local selected = navigation.selected(value, state)
    if selected == "patch_cards" then patch_cards.update(value, assets, state, action)
    elseif selected == "navicust" then navicust.update(value, assets, state, action)
    else folder.update(value, assets, state, action) end
    return nil
end

local templates = require("./save/templates")
local editor: Editor = {
    templates = templates.list,
    create_save = templates.create,
    detect_rom = require("../rom/detect"),
    decode = decode,
    encode = save.encode,
    validate = require("./build").validate,
    view = view,
    update = update,
    reset_view = session.reset_view,
    test = require("./test").test,
}
return editor
