--!strict
local save = require("./init")
local codec = require("@bn-common/save/codec")
local references = require("./fixtures/references")

-- Build legacy inputs independently of the decoder's normalization. Every
-- input and normalized result also has a checksum from the native reference.
local function shifted(input: buffer, game: save.Info, shift: number, first: number): buffer
    local bytes = buffer.fromstring(buffer.tostring(input))
    codec.mask(bytes, 0, save.SIZE, 0x1554)
    buffer.writeu8(bytes, 0, first)
    buffer.copy(bytes, 0x2130 + shift, bytes, 0x2130, 0x5e20 - 0x2130)
    buffer.fill(bytes, 0x2130, 0, shift)
    buffer.writeu32(bytes, 0x1550, shift)
    local checksum = codec.checksum(bytes, 0, save.SIZE, 0x21e8 + shift)
        + (if game.variant == "RedSun" then 0x16 else 0x22) - (if game.region == "JP" then first else 0)
    buffer.writeu32(bytes, 0x21e8 + shift, checksum)
    codec.mask(bytes, 0, save.SIZE, 0x1554)
    return bytes
end

return function()
    local invalid_size = tango.input_size("test.invalid_save")
    local invalid = if invalid_size then buffer.readu8(tango.read_input("test.invalid_save", 0, 1), 0) else nil
    if invalid == 1 then save.decode(buffer.create(save.SIZE - 1)); error("accepted short save") end
    local fixture_size = tango.input_size("test.fixture")
    local name = if fixture_size then buffer.tostring(tango.read_input("test.fixture", 0, fixture_size)) else references[1].name
    for _, reference in references do
        if reference.name ~= name then continue end
        local raw = tango.read_file("editor/save/templates/" .. reference.name .. ".raw")
        local bytes = save.from_wram(raw, reference.info)
        local before = buffer.tostring(bytes)
        local encoded = save.encode(bytes)
        assert(buffer.tostring(bytes) == before)
        assert(tango.crc32(encoded) == reference.crc32, reference.name)
        if invalid then
            if invalid == 2 then
                buffer.writeu8(encoded, 0x40, bit32.bxor(buffer.readu8(encoded, 0x40), 1))
            elseif invalid == 3 or invalid == 4 then
                encoded = shifted(encoded, reference.info, if invalid == 3 then 2 else 0x200, 1)
            elseif invalid == 5 then
                buffer.writeu8(encoded, 0x2208, bit32.bxor(buffer.readu8(encoded, 0x2208), 1))
            elseif invalid == 6 then
                buffer.writeu8(bytes, 0, 0)
                save.encode(bytes)
                error("accepted invalid document identity")
            end
            save.decode(encoded)
            error("accepted invalid save")
        end
        for _, case in reference.shifted do
            local input = shifted(encoded, reference.info, case.shift, case.first)
            assert(tango.crc32(input) == case.input_crc, reference.name)
            -- The native exporter clears unrelated SRAM. The package preserves
            -- it, including when normalizing a shifted save region.
            buffer.writeu8(input, save.SIZE, 0xab)
            buffer.writeu8(input, 65535, 0xcd)
            local original = buffer.tostring(input)
            local decoded = save.decode(input)
            assert(buffer.tostring(input) == original)
            local game = save.info(decoded)
            assert(game.variant == reference.info.variant)
            assert(game.region == (if case.first == 0 then "ANY" else reference.info.region))
            assert(buffer.readu32(decoded, save.START + 0x1550) == 0)
            local normalized = save.encode(decoded)
            assert(buffer.readu8(normalized, save.SIZE) == 0xab and buffer.readu8(normalized, 65535) == 0xcd)
            buffer.writeu8(normalized, save.SIZE, 0)
            buffer.writeu8(normalized, 65535, 0)
            assert(tango.crc32(normalized) == case.normalized_crc, reference.name .. " shift " .. tostring(case.shift))
            -- Changing payload bytes cannot lose the identity used for encoding.
            buffer.writeu8(decoded, save.START + 0x40, 23)
            local edited = save.decode(save.encode(decoded))
            assert(save.info(edited).variant == game.variant and save.info(edited).region == game.region)
            assert(buffer.readu8(edited, save.START + 0x40) == 23)
        end
    end
end
