--!strict
local rom = require("./init")

local function test()
    -- Synthetic cartridge: pointers, messages, WRAM substitutions and graphics.
    local bytes, wram = buffer.create(0x50000), buffer.create(0x7c14)
    buffer.writestring(bytes, 0xac, "BRBE")
    buffer.writeu32(bytes, 0x4a0f0, 0x08045000)
    buffer.writeu32(bytes, 0x40b84, 0x08043000)
    buffer.writeu32(bytes, 0x23b40, 0x08044000)
    buffer.writeu16(bytes, 0x43000, 4)
    buffer.writeu16(bytes, 0x43002, 5)
    buffer.writestring(bytes, 0x43004, "\xe6\x0b\xe9\xe7\x00\xe8\x04\x00\x00\xfa\x03\x00\x01\xff\x00\x00\xe6")
    buffer.writeu16(bytes, 0x44000, 4)
    buffer.writeu16(bytes, 0x44002, 5)
    buffer.writestring(bytes, 0x44004, "\xe6\xff\x01\x00\xe6")
    buffer.writestring(wram, 0x1376, "\x0c\xe6")
    buffer.writestring(wram, 0x1d16, "\x0d\xe6")
    local chip = 0x1e214 + 0x2c
    buffer.writestring(bytes, chip, "\x01\x1a\xff\xff")
    buffer.writeu8(bytes, chip + 6, 3)
    buffer.writeu8(bytes, chip + 7, 1)
    buffer.writeu8(bytes, chip + 8, 40)
    buffer.writeu8(bytes, chip + 9, 0x20)
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
    assert(c and c.name == "A\nC" and c.description == "B")
    assert(c.codes[1] == 1 and c.codes[2] == 26 and #c.codes == 2)
    assert(c.element == 3 and c.class == "mega" and c.mb == 40 and c.attack == 0 and c.order == 11)
    assert(c.dark and c.legal and assets.chip(368) == nil and assets.chip(-1) == nil)
    assert(buffer.readu32(c.icon().rgba, 0) == 0xff0000ff)
    assert(buffer.readu32(c.icon().rgba, 4) == 0)
    assert(buffer.readu32(c.image().rgba, 4) == 0xff0000ff)
    buffer.writeu32(bytes, chip + 0x20, 0)
    local broken = rom.open(bytes, wram, nil).chip(1)
    assert(broken and buffer.readu32(broken.icon().rgba, 0) == 0)

end

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
    assert(text() == "bn5-rom-v1")
    local rom_size = tango.input_size("rom")
    assert(rom_size)
    local input = tango.read_input("rom", 0, rom_size)
    assert(text() == buffer.readstring(input, 0xac, 4))
    local fixture = text()
    assert(fixture)
    local assets = rom.open(input, tango.read_file("editor/save/fixtures/" .. fixture .. ".raw"), nil)
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
        for _, compressed in {false, true} do
            local bitmap = part.bitmap(compressed)
            assert(bitmap and #bitmap == 25, label .. " bitmap size")
            for _, filled in bitmap do assert(filled == (u32() ~= 0), label .. " bitmap") end
        end
    end
    for _ = 1, u32() do
        local id = u32()
        local label = "Patch Card " .. tostring(id)
        local card = assets.patch_card(id)
        assert(card, label)
        assert(card.name == text(), label .. " name")
        assert(card.mb == u32(), label .. " MB")
        assert(#card.effects == u32(), label .. " effect count")
        for _, effect in card.effects do
            assert(effect.id == u32(), label .. " effect ID")
            assert(effect.kind == text(), label .. " effect kind")
            assert(effect.name == text(), label .. " effect name")
            assert(effect.parameter == u32(), label .. " effect parameter")
            assert(effect.ability == (u32() ~= 0), label .. " ability")
            assert(effect.debuff == (u32() ~= 0), label .. " debuff")
        end
    end
    for _ = 1, u32() do
        local id = u32()
        local navi = assets.navi(id)
        assert(navi and navi.name == text(), "Navi name " .. tostring(id))
        image(navi.emblem, "Navi emblem " .. tostring(id))
    end
    assert(cursor == length, "Unused native reference bytes")
end
return {test = test, compare = compare}
