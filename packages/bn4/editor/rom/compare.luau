--!strict
local rom = require("./init")

local function compare()
    local length = tango.input_size("reference.rom")
    if not length then return end
    local expected = tango.read_input("reference.rom", 0, length)
    local cursor = 0
    local function u32(): number
        local n = buffer.readu32(expected, cursor); cursor += 4; return n
    end
    local function text(): string?
        local size = u32()
        if size == 0xffffffff then return nil end
        local value = buffer.readstring(expected, cursor, size); cursor += size; return value
    end
    local function image(actual: Raster, label: string)
        assert(actual.width == u32() and actual.height == u32(), label .. " dimensions")
        local size = u32()
        assert(buffer.len(actual.rgba) == size)
        assert(buffer.tostring(actual.rgba) == buffer.readstring(expected, cursor, size), label .. " pixels")
        cursor += size
    end
    assert(text() == "bn4-rom-v1")
    local rom_size = tango.input_size("rom")
    assert(rom_size)
    local input = tango.read_input("rom", 0, rom_size)
    assert(text() == buffer.readstring(input, 0xac, 4))
    local fixture = text()
    assert(fixture)
    local assets = rom.open(input, tango.read_file("editor/save/templates/" .. fixture .. ".raw"), nil)
    for _ = 1, u32() do
        local id = u32()
        local label = "chip " .. tostring(id)
        local item = assets.chip(id)
        assert(item, label)
        assert(item.name == text(), label .. " name")
        assert(item.description == text(), label .. " description")
        assert(#item.codes == u32(), label .. " codes")
        for _, code in item.codes do assert(code == u32(), label .. " code") end
        assert(item.element == u32(), label .. " element")
        assert(item.class == text(), label .. " class")
        assert(item.mb == u32(), label .. " MB")
        assert(item.attack == u32(), label .. " attack")
        assert(item.order == u32(), label .. " order")
        assert(item.legal == (u32() == 1), label .. " legality")
        assert(item.dark == (u32() == 1), label .. " dark")
        image(item.icon(), label .. " icon"); image(item.image(), label .. " image")
    end
    for _ = 1, u32() do
        local id = u32()
        local icon = assets.element_icon(id)
        assert(icon); image(icon, "element " .. tostring(id))
    end
    for _ = 1, u32() do
        local id = u32()
        local label = "NaviCust part " .. tostring(id)
        local part = assets.navicust_part(id)
        assert(part, label)
        assert(part.name == text(), label .. " name")
        assert(part.description == text(), label .. " description")
        assert(part.color_id == u32(), label .. " color")
        assert(part.solid == (u32() ~= 0), label .. " solid")
        assert(part.color == text(), label .. " color name")
        assert(part.bitmap(true) == nil, label .. " compressed bitmap")
        do
            local bitmap = part.bitmap(false)
            assert(bitmap and #bitmap == 25, label .. " bitmap size")
            for _, filled in bitmap do assert(filled == (u32() ~= 0), label .. " bitmap") end
        end
    end
    assert(cursor == length, "Unused native reference bytes")
end
return compare
