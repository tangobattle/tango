--!strict
local save = require("./save")
local model = require("./save/model")
local rom = require("./rom")
local navigation = require("./navigation")
local grid = require("@bn-common/navicust/ui/grid")
local t = require("./i18n")

local function find(node: Node, id: string): Node?
    if node.kind == "button" or node.kind == "input" or node.kind == "canvas" or node.kind == "checkbox" then
        if node.id == id then return node end
    end
    if node.kind == "column" or node.kind == "row" or node.kind == "stack" or node.kind == "list" or node.kind == "wrap" then
        for _, child in node.children do
            local found = find(child, id)
            if found then return found end
        end
    elseif node.kind == "container" or node.kind == "scroll" then
        local child = node.child
        if child then return find(child, id) end
    elseif node.kind == "button" then
        local child = node.child
        if child then return find(child, id) end
    end
    return nil
end

return function(editor: Editor, fixture_name: string)
    -- Exercise the actual exported editor with a small synthetic catalog. The
    -- public ROM method is also the extension point used by dependent packages.
    local original_reader = rom.from_inputs
    local exercised = false
    for _, fixture in require("./save/fixtures/references") do
        if fixture.name ~= fixture_name then continue end
        exercised = true
        local bytes = buffer.create(65536)
        buffer.copy(bytes, save.START, tango.read_file("editor/save/fixtures/" .. fixture.name .. ".raw"))
        local assets = require("./tests/assets")(save.info(bytes))
        rom.from_inputs = function(_wram: buffer?): rom.Assets? return assets end
        local value = model.open(bytes)
        local state: ViewState = {}
        local context: EditorContext = {read_only = true,
            session = {editable = true, editing = false, saving = false, can_save = false},
            embedding = {inline_actions = true, streamer_mode = false, actions = {play = {enabled = true, while_editing = false}}}}
        local function activate(id: string, action_value: string?): {Effect}?
            local node = find(editor.view(bytes, state, context), id)
            assert(node and node.kind == "button" and node.enabled ~= false and node.action_enabled ~= false, id)
            return editor.update(bytes, state, {kind = "activate", id = id, value = action_value or ""}, context)
        end
        local tabs = navigation.available(value)
        assert(#tabs == 4 and tabs[1] == "navicust" and tabs[4] == "auto_battle_data")
        local before = buffer.tostring(bytes)
        for _, tab in tabs do
            activate("editor.tab:" .. tab)
            assert(navigation.selected(value, state) == tab)
            local effects = activate("editor.copy:" .. tab)
            assert(effects and #effects == 1)
            assert(effects[1].kind == (if tab == "folder" then "copy_html" else "copy_text"))
            assert(not find(editor.view(bytes, state, context), "navi.picker"))
        end
        assert(buffer.tostring(bytes) == before, "Viewing and copying cannot mutate the save")
        context.read_only = false
        context.session = {editable = true, editing = true, saving = false, can_save = true}
        activate("editor.tab:folder")
        activate("folder.clear")
        activate("folder.add:1:0", "1:0")
        activate("folder.regular:1")
        local chip = value.chip(value.equipped_folder(), 1)
        assert(chip and chip.id == 1 and value.regular_index(value.equipped_folder()) == 1)
        assert(not find(editor.view(bytes, state, context), "folder.tag:1"), "BN5 has no Tag Chips")
        activate("editor.tab:auto_battle_data")
        activate("auto_battle_data.clear")
        assert(find(editor.view(bytes, state, context), "auto_battle_data.used:1"))
        editor.update(bytes, state, {kind = "change", id = "auto_battle_data.used:1", value = "99x9999"}, context)
        assert(value.chip_use_count(1, false) == 65535 and value.auto_battle_data()[4].id == 1)
        activate("editor.tab:navicust")
        activate("navicust.clear")
        activate("navicust.pick:4")
        activate("navicust.rotate:4")
        local geometry = grid.geometry({cols = 5, rows = 5})
        local x, y = geometry.origin_x + 2.5 * geometry.cell, geometry.origin_y + 2.5 * geometry.cell
        editor.update(bytes, state, {kind = "pointer", id = "navicust.board", phase = "down", button = "left", x = x, y = y,
            dx = 0, dy = 0, modifiers = {shift = false, control = false, alt = false, command = false}}, context)
        local placed = value.navicust_part(1)
        assert(placed and placed.id == 4 and placed.col == 2 and placed.row == 2 and placed.rot == 1 and placed.compressed)
        local materialized = value.materialized()
        assert(materialized and materialized[13] == 1 and state["navicust.held.id"] == nil)
        activate("editor.tab:patch_cards")
        activate("patch_cards.clear")
        activate("patch_cards.add:1")
        activate("patch_cards.add:2")
        assert(value.patch_count() == 2 and #editor.validate(bytes) > 0)
        local report = require("./build").inspect(bytes, assets)
        assert(report.patch_mb == 90 and #report.patch_cards > 0)
        editor.update(bytes, state, {kind = "reorder", id = "patch_cards.slots", from = 1, to = 2}, context)
        local card = value.patch_card(1)
        assert(card and card.id == 2)
        activate("patch_cards.remove:1")
        card = value.patch_card(1)
        assert(value.patch_count() == 1 and card and card.id == 1)
        activate("navi.picker")
        local link = assets.definition.navi_order[1][2]
        activate("navi.select:" .. tostring(link))
        tabs = navigation.available(value)
        assert(#tabs == 2 and tabs[1] == "folder" and tabs[2] == "auto_battle_data")
        assert(not find(editor.view(bytes, state, context), "editor.tab:patch_cards"))
        activate("navi.picker")
        activate("navi.select:0")
        assert(#navigation.available(value) == 4)
        -- Preferences survive edit transitions; drafts and held parts do not.
        state["auto_battle_data.sort"], state["auto_battle_data.filter"] = "used", "pending"
        state["navicust.held.id"], state["navi.picker"] = "4", "open"
        local reset = editor.reset_view
        assert(reset)
        reset(state)
        assert(state["auto_battle_data.sort"] == "used" and not state["auto_battle_data.filter"])
        assert(not state["navicust.held.id"] and not state["navi.picker"])
        context.read_only = true
        context.embedding.streamer_mode = true
        local cover = editor.view(bytes, state, context)
        assert(find(cover, "editor.review") and not find(cover, "editor.tab:folder"))
        activate("editor.review")
        assert(find(editor.view(bytes, state, context), "editor.tab:folder"))
    end
    assert(exercised, "Unknown editor fixture: " .. fixture_name)
    rom.from_inputs = function(_wram: buffer?): rom.Assets? return nil end
    local raw = tango.read_file("editor/save/fixtures/light_protoman_us.raw")
    local bytes = buffer.create(65536)
    buffer.copy(bytes, save.START, raw)
    local node = editor.view(bytes, {}, {read_only = true,
        embedding = {inline_actions = true, streamer_mode = false, actions = {}}})
    assert(node.kind == "text" and node.text == t("rom-required"))
    rom.from_inputs = original_reader
end
