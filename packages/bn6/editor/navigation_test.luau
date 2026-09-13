--!strict
local navigation = require("./navigation")
local model = require("./save/model")
local save = require("./save")

local function test()
    for _, name in {"g_us", "f_us", "g_jp", "f_jp"} do
        local bytes = buffer.create(65536)
        buffer.copy(bytes, save.START, tango.read_file("editor/save/fixtures/" .. name .. ".raw"))
        local value = model.open(bytes)
        assert(value.set_navi(0))
        local state: ViewState = {}
        local choices = navigation.available(value)
        assert(choices[1] == "navicust" and choices[2] == "folder")
        assert((choices[3] == "patch_cards") == (save.info(bytes).region == "JP"))
        assert(navigation.selected(value, state) == "navicust")
        state["editor.tab"] = "missing"
        assert(navigation.selected(value, state) == "navicust")
        state["editor.tab"], state["navi.picker"] = "navicust", "open"
        state["navicust.viewer.col"], state["navicust.hover.row"] = "1", "2"
        state["folder.filter"] = "kept"
        local before = buffer.tostring(bytes)
        local effects = navigation.update(value, state, {kind = "scroll", id = "editor.tabs", x = 0.75, y = 0})
        assert(effects and #effects == 0 and state["editor.tab_scroll"] == "0.75")
        effects = navigation.update(value, state, {kind = "activate", id = "editor.tab:folder", value = ""})
        assert(effects and #effects == 2)
        local body, strip = effects[1], effects[2]
        assert(body.kind == "scroll_to" and body.id == "editor.body" and body.x == 0 and body.y == 0)
        assert(strip.kind == "scroll_to" and strip.id == "editor.tabs" and strip.x == 0 and strip.y == 0)
        assert(navigation.selected(value, state) == "folder" and state["editor.prev_tab"] == "navicust")
        assert(tostring(state["editor.tab_scroll"]) == "0" and not state["navi.picker"])
        assert(not state["navicust.viewer.col"] and not state["navicust.hover.row"])
        assert(state["folder.filter"] == "kept" and buffer.tostring(bytes) == before)
        assert(state["editor.enter.revision"] == "1" and state["editor.enter.x"] == "24")
        navigation.update(value, state, {kind = "activate", id = "editor.tab:folder", value = ""})
        assert(state["editor.enter.revision"] == "1" and state["editor.prev_tab"] == "navicust")
        navigation.update(value, state, {kind = "activate", id = "editor.tab:navicust", value = ""})
        assert(tostring(state["editor.enter.revision"]) == "2" and tostring(state["editor.enter.x"]) == "-24")
        local motion = require("@editor-common/navigation_motion")
        local resting: Node = {kind = "space"}
        assert(motion.tail(resting, state, "copy", true).motion == nil)
        local arrival = motion.tail(resting, state, "image", false).motion
        assert(arrival and arrival.from[1] == -24 and arrival.from[2] == 0 and resting.motion == nil)
        require("@bn-common/editor/session").reset_view(state)
        assert(tostring(state["editor.enter.revision"]) == "3" and tostring(state["editor.enter.x"]) == "0")
        assert(value.set_navi(1))
        assert(navigation.available(value)[1] == "folder")
        state["editor.tab"] = "navicust"
        assert(navigation.selected(value, state) == "folder")
        assert(navigation.update(value, state, {kind = "activate", id = "editor.tab:navicust", value = ""}) == nil)
    end
end

return {test = test}
