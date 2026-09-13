--!strict
local rom = require("./init")
local function test()
    local length = tango.input_size("reference.parts")
    if not length then return end
    local expected = tango.read_input("reference.parts", 0, length)
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
    local function bitmap(actual: {boolean}?)
        local present = u32() == 1
        assert((actual ~= nil) == present)
        if actual then
            assert(#actual == 49)
            for _, cell in actual do assert(cell == (u32() == 1)) end
        end
    end
    assert(string_value() == "tango.bn6-parts-reference-v1")
    local size = tango.input_size("rom")
    assert(size)
    local bytes = tango.read_input("rom", 0, size)
    assert(string_value() == buffer.readstring(bytes, 0xac, 4))
    local assets = rom.open(bytes, nil, nil)
    assert(u32() == 188)
    for _ = 0, 187 do
        local id = u32()
        local part = assets.navicust_part(id)
        assert(part)
        assert(part.name == string_value(), "NaviCust name " .. tostring(id))
        assert(part.description == string_value(), "NaviCust description " .. tostring(id))
        assert(part.color == string_value() and part.solid == (u32() == 1), "NaviCust attributes " .. tostring(id))
        bitmap(part.bitmap(false))
        bitmap(part.bitmap(true))
        -- Caching must not share mutable footprints with callers.
        for _, compressed in {false, true} do
            local first = part.bitmap(compressed)
            local second = part.bitmap(compressed)
            if first and second then
                assert(first ~= second)
                local original = second[1]
                first[1] = not first[1]
                local third = part.bitmap(compressed)
                assert(third and third[1] == original)
            end
        end
    end
    assert(u32() == 12)
    for id = 0, 11 do
        local navi = assets.navi(id)
        assert((navi ~= nil) == (u32() == 1), "Navi roster " .. tostring(id))
        if navi then
            assert(navi.name == string_value(), "Navi name " .. tostring(id))
            assert(navi.emblem.width == u32() and navi.emblem.height == u32())
            local emblem_length = u32()
            assert(buffer.tostring(navi.emblem.rgba) == buffer.readstring(expected, cursor, emblem_length), "Navi emblem " .. tostring(id))
            cursor += emblem_length
        end
    end
    local count = u32()
    for _ = 1, count do
        local id = u32()
        local card = assets.patch_card(id)
        assert(card)
        assert(card.name == string_value() and card.mb == u32(), "Patch card " .. tostring(id))
        assert(#card.effects == u32())
        for _, effect in card.effects do
            assert(effect.id == u32() and effect.kind == string_value())
            assert(effect.name == string_value(), "Patch card effect " .. tostring(id) .. ":" .. tostring(effect.id))
            assert(effect.parameter == u32() and effect.ability == (u32() == 1) and effect.debuff == (u32() == 1))
        end
    end
    assert(cursor == length)
    assert(assets.patch_card(0) == nil and assets.patch_card(118) == nil)
    assert(assets.navicust_part(188) == nil and assets.navi(12) == nil)
end
return {test = test}
