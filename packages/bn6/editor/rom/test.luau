--!strict
local rom = require("./init")

local function test()
    -- Synthetic cartridge: pointers, messages, WRAM substitutions and graphics.
    local bytes, wram = buffer.create(0x50000), buffer.create(0x6710)
    buffer.writestring(bytes, 0xac, "BR5E")
    buffer.writeu32(bytes, 0x1ed20, 0x08045000)
    buffer.writeu32(bytes, 0x42038, 0x08043000)
    buffer.writeu32(bytes, 0x27d50, 0x08044000)
    buffer.writeu16(bytes, 0x43000, 4)
    buffer.writeu16(bytes, 0x43002, 5)
    buffer.writestring(bytes, 0x43004, "\xe6\x0b\xe9\xe7\x00\xe8\x04\x00\x00\xfa\x03\x00\x01\xe6")
    buffer.writeu16(bytes, 0x44000, 4)
    buffer.writeu16(bytes, 0x44002, 5)
    buffer.writestring(bytes, 0x44004, "\xe6\xff\x01\x00\xe6")
    buffer.writestring(wram, 0x07d6, "\x0c\xe6")
    local chip = 0x21da8 + 0x2c
    buffer.writestring(bytes, chip, "\x01\x1a\xff\xff")
    buffer.writeu8(bytes, chip + 6, 3)
    buffer.writeu8(bytes, chip + 7, 1)
    buffer.writeu8(bytes, chip + 8, 40)
    buffer.writeu16(bytes, chip + 0x1a, 1000)
    buffer.writeu16(bytes, chip + 0x1c, 11)
    buffer.writeu32(bytes, chip + 0x20, 0x08045100)
    buffer.writeu32(bytes, chip + 0x24, 0x08045200)
    buffer.writeu32(bytes, chip + 0x28, 0x08045000)
    buffer.writeu16(bytes, 0x45002, 31)
    buffer.writeu8(bytes, 0x45100, 1)
    buffer.writeu8(bytes, 0x45200, 0x11)
    local assets = rom.open(bytes, wram, nil)
    local c = assets.chip(1)
    assert(c and c.name == "A\n" and c.description == "B")
    assert(c.codes[1] == 1 and c.codes[2] == 26 and #c.codes == 2)
    assert(c.element == 3 and c.class == "mega" and c.mb == 40 and c.attack == 0 and c.order == 11)
    assert(c.legal and assets.chip(411) == nil and assets.chip(-1) == nil)
    assert(buffer.readu32(c.icon().rgba, 0) == 0xff0000ff)
    assert(buffer.readu32(c.icon().rgba, 4) == 0)
    assert(buffer.readu32(c.image().rgba, 4) == 0xff0000ff)
    buffer.writeu32(bytes, chip + 0x20, 0)
    local broken = rom.open(bytes, wram, nil).chip(1)
    assert(broken and buffer.readu32(broken.icon().rgba, 0) == 0)

    local length = tango.input_size("reference")
    if not length then return end
    -- A local reference exported by the native dataview. ROM-derived bytes
    -- remain outside the package; compare every field and every RGBA pixel.
    local expected = tango.read_input("reference", 0, length)
    local cursor = 0
    local function u32(): number
        local n = buffer.readu32(expected, cursor)
        cursor += 4
        return n
    end
    local function string_value(): string?
        local size = u32()
        if size == 0xffffffff then return nil end
        local value = buffer.readstring(expected, cursor, size)
        cursor += size
        return value
    end
    local function image_value(actual: Raster)
        assert(actual.width == u32() and actual.height == u32(), "Image dimensions differ.")
        local size = u32()
        assert(buffer.len(actual.rgba) == size)
        assert(buffer.tostring(actual.rgba) == buffer.readstring(expected, cursor, size), "Image pixels differ.")
        cursor += size
    end
    assert(string_value() == "tango.bn6-rom-reference-v1")
    local input_length = tango.input_size("rom")
    assert(input_length, "ROM required with reference data.")
    local input = tango.read_input("rom", 0, input_length)
    assert(string_value() == buffer.readstring(input, 0xac, 4), "Reference ROM variant differs.")
    local actual = rom.open(input, tango.read_file("editor/save/fixtures/" ..
        (if buffer.readstring(input, 0xac, 3) == "BR5" then "g" else "f") ..
        (if buffer.readstring(input, 0xaf, 1) == "J" then "_jp" else "_us") .. ".raw"), nil)
    local count = u32()
    assert(count >= 1 and count <= 16, "Compare at most 16 chips per operation.")
    for _ = 1, count do
        local id = u32()
        local item = actual.chip(id)
        assert(item, "Missing chip " .. tostring(id))
        assert(item.name == string_value(), "Chip name differs: " .. tostring(id))
        assert(item.description == string_value(), "Chip description differs: " .. tostring(id))
        assert(#item.codes == u32(), "Chip codes differ: " .. tostring(id))
        for _, code in item.codes do assert(code == u32()) end
        assert(item.element == u32() and item.class == string_value())
        assert(item.mb == u32() and item.attack == u32() and item.order == u32())
        assert(item.legal == (u32() == 1))
        image_value(item.icon())
        image_value(item.image())
    end
    for id = 0, 10 do
        local icon = actual.element_icon(id)
        assert(icon)
        image_value(icon)
    end
    assert(cursor == length, "Unused reference bytes.")
end

return {test = test}
