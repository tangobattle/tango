--!strict
local save = require("../save")
local model = require("../save/model")
local rom = require("../rom")
local definitions = require("../rom/offsets").definitions
local controller = require("./ui")
local motion = require("@editor-common/navigation_motion")

local function test()
    local variants: {[string]: string} = {g_us = "BR5E", f_us = "BR6E", g_jp = "BR5J", f_jp = "BR6J"}
    for name, code in variants do
        local bytes = buffer.create(65536)
        buffer.copy(bytes, save.START, tango.read_file("editor/save/fixtures/" .. name .. ".raw"))
        local value = model.open(bytes)
        local definition = definitions[code]
        local assets: rom.Assets = {definition = definition, navicust_command_line = 3,
            chip = function(_id: number) return nil end, chip_is_legal = function(_id: number): boolean return false end,
            element_icon = function(_id: number) return nil end, navicust_part = function(_id: number) return nil end,
            patch_card = function(_id: number) return nil end, navi = function(_id: number) return nil end}
        local state: ViewState = {["editor.tab"] = "navicust", ["folder.tags"] = "4", ["navicust.held.id"] = "1",
            ["navicust.held.rot"] = "2", ["navicust.held.compressed"] = "false"}
        local original = buffer.tostring(bytes)
        assert(not controller.update(value, assets, state, {id = "navi.select:0", kind = "activate", value = ""}))
        assert(controller.update(value, assets, state, {id = "navi.picker", kind = "activate", value = ""}))
        assert(state["navi.picker"] == "open")
        local revision = state["editor.enter.revision"]
        local node: Node = {kind = "space"}
        assert(motion.body(node, state).motion == nil)
        local arrival = motion.region(node, state).motion
        assert(arrival and arrival.from[1] == 0 and arrival.from[2] == 20)
        local unavailable = if definition.variant == "Gregar" then 6 else 1
        for _, id in {tostring(unavailable), "-1", "0.5", "12", "bad"} do
            assert(not controller.update(value, assets, state, {id = "navi.select:" .. id, kind = "activate", value = ""}))
        end
        assert(buffer.tostring(bytes) == original and state["navi.picker"] == "open")
        assert(state["editor.enter.revision"] == revision)
        assert(controller.update(value, assets, state, {id = "navi.picker", kind = "activate", value = ""}))
        assert(state["navi.picker"] == nil and buffer.tostring(bytes) == original)
        assert(state["editor.enter.revision"] ~= revision and motion.region(node, state).motion ~= nil)
        for _, row in definition.navi_order do
            for _, id in row do
                assert(controller.update(value, assets, state, {id = "navi.picker", kind = "activate", value = ""}))
                assert(controller.update(value, assets, state, {id = "navi.select:" .. tostring(id), kind = "activate", value = ""}))
                assert(value.navi() == id and state["navi.picker"] == nil)
                assert(value.has_navicust() == (id == 0))
                assert((value.patch_count() ~= nil) == (id == 0 and definition.region == "JP"))
                assert(state["editor.tab"] == "navicust" and state["folder.tags"] == "4" and state["navicust.held.id"] == "1")
                assert(state["navicust.held.rot"] == "2" and state["navicust.held.compressed"] == "false")
            end
        end
        -- Tab/edit/save entrances take over from the picker without layering
        -- two transforms or retaining a previous picker revision.
        motion.restart(state, -24)
        assert(motion.region(node, state).motion == nil)
        local body = motion.body(node, state).motion
        assert(body and body.from[1] == -24 and body.from[2] == 0)
    end
end

return {test = test}
