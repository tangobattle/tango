--!strict
-- Host-level fixture for the Mod Card component while the full editor is ported.
local save = require("@bn4/editor/save")
local model = require("@bn4/editor/save/patch_cards")
local catalog = require("@bn4/editor/patch_cards/catalog")
local ui = require("@bn4/editor/patch_cards/ui")
local controller = require("@bn4/editor/patch_cards/state")
local rules = require("@bn4/editor/patch_cards/rules")
local labels = require("@bn4/editor/patch_cards/labels")
local t = require("@bn4/editor/i18n")

local function assets(bytes: buffer): controller.Assets
    local region = save.info(bytes).region
    return {patch_card = function(id: number): catalog.Card? return catalog.get(id, if region == "JP" then "JP" else "US") end}
end
local editor: Editor = {
    templates = function(): {SaveTemplate} return {{name = "fixture", label = "Fixture"}} end,
    create_save = function(_name: string): buffer
        local size = tango.input_size("template")
        local name_size = tango.input_size("template_name")
        assert(size and name_size)
        local name = buffer.tostring(tango.read_input("template_name", 0, name_size))
        return save.encode(save.from_wram(tango.read_input("template", 0, size), {
            region = if string.sub(name, -3) == "_jp" then "JP" else "US",
            variant = if string.find(name, "_rs_") then "RedSun" else "BlueMoon",
        }))
    end,
    decode = save.decode, encode = save.encode,
    view = function(bytes: buffer, _state: ViewState, context: EditorContext): Node
        return ui.view(model.open(bytes), assets(bytes), context.read_only)
    end,
    validate = function(bytes: buffer): {string}
        local provider = assets(bytes)
        return labels.warnings(rules.violations(model.open(bytes), provider), provider)
    end,
    update = function(bytes: buffer, _state: ViewState, action: Action, context: EditorContext): {Effect}?
        if context.read_only or not controller.update(model.open(bytes), assets(bytes), action) then
            return tango.fail(t, "unknown-action")
        end
        return nil
    end,
}
return editor
