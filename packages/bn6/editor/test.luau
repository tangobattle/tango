--!strict
local save = require("./save")
local START, SIZE = save.START, save.SIZE
local info, decode, encode, chip_offset = save.info, save.decode, save.encode, save.chip_offset
local model = require("./save/model")
local function test()
    if tango.input_size("reference.folder") then require("./folder/test").test(); return end
    if tango.input_size("reference.build") then require("./build/test").test(); return end
    if tango.input_size("reference.model") then require("./save/model_test").test(); return end
    if tango.input_size("reference.parts") then require("./rom/parts_test").test(); return end
    require("./rom/test").test()
    if tango.input_size("reference") then return end
    require("./build/test").test()
    require("./save/model_test").test()
    require("./navi/test").test()
    require("./navigation_test").test()
    require("./session_test").test()
    require("./cover/test").test()
    local state: ViewState = {["editor.tab"] = "navicust", ["editor.prev_tab"] = "folder", ["editor.tab_scroll"] = "0.75",
        ["editor.reviewing"] = "true",
        ["folder.grouped"] = "false", ["folder.sort"] = "name",
        ["navicust.sort"] = "color", ["patch_cards.sort"] = "mb", ["navi.picker"] = "open",
        ["folder.filter"] = "pending", ["navicust.held.id"] = "4", ["patch_cards.filter"] = "pending"}
    require("@bn-common/editor/session").reset_view(state)
    assert(state["editor.tab"] == "navicust" and state["folder.grouped"] == "false" and state["folder.sort"] == "name")
    assert(state["navicust.sort"] == "color" and state["patch_cards.sort"] == "mb")
    assert(state["editor.prev_tab"] == "folder" and state["editor.tab_scroll"] == "0.75")
    assert(state["editor.reviewing"] == "true")
    local count = 0
    for _ in state do count += 1 end
    assert(count == 10, "Edit transitions retain view preferences and restart presentation while discarding scratch state")
    for _, name in {"g_us", "f_us", "g_jp", "f_jp"} do
        local raw = tango.read_file("editor/save/fixtures/" .. name .. ".raw")
        local original = buffer.create(65536)
        buffer.copy(original, START, raw, 0, SIZE)
        -- Preserve unrelated SRAM bytes, including data outside the save region.
        buffer.writeu8(original, 0, 0xab)
        buffer.writeu8(original, 65535, 0xcd)
        local encoded = encode(original)
        local bytes = decode(encoded)
        -- Format roundtrips cover every cartridge, even when this test VM
        -- has one particular ROM bound. Build parity uses matching inputs.
        assert(#save.validate(bytes) == 0)
        assert(buffer.tostring(encode(bytes)) == buffer.tostring(encoded))
        local value = model.open(bytes)
        assert(value.set_chip(1, 1, {id = 1, code = 26}))
        value.rebuild_chip_anticheat()
        local reread = decode(encode(bytes))
        assert(buffer.readu16(reread, chip_offset(1, 1)) == 1 + 26 * 512)
        assert(buffer.readu8(reread, 0) == 0xab and buffer.readu8(reread, 65535) == 0xcd)
        local game = info(reread)
        local shift = if game.region == "JP" then 0x40 else 0
        local key = if game.variant == "Gregar" then 0x17 else 0x81
        assert(buffer.readu8(reread, START + 0x4c20 - shift)
            == bit32.bxor(buffer.readu8(reread, START + 0x08a0), key))
    end
end

return {test = test}
