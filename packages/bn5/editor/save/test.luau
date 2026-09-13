--!strict
local save = require("./init")
local references = require("./fixtures/references")

return function()
    local invalid_size = tango.input_size("test.invalid_save")
    local invalid = if invalid_size then buffer.readu8(tango.read_input("test.invalid_save", 0, 1), 0) else nil
    if invalid == 1 then
        save.decode(buffer.create(save.START + save.SIZE - 1))
        return
    end
    for _, reference in references do
        local raw = tango.read_file("editor/save/fixtures/" .. reference.name .. ".raw")
        assert(buffer.len(raw) == save.SIZE)
        local original = buffer.create(65536)
        buffer.copy(original, save.START, raw)
        local before = buffer.tostring(original)
        local encoded = save.encode(original)
        if invalid == 2 then
            buffer.writeu8(encoded, save.START, bit32.bxor(buffer.readu8(encoded, save.START), 1))
            save.decode(encoded)
            return
        elseif invalid == 3 then
            buffer.writeu32(original, save.START + 0x1a30, 4)
            save.decode(save.encode(original))
            return
        end
        assert(buffer.tostring(original) == before)
        -- This comes from the existing native encoder, not from a scripted
        -- encode/decode roundtrip. All eight light/dark regional saves agree.
        assert(tango.crc32(encoded) == reference.crc32, reference.name)
        local bytes = save.decode(encoded)
        assert(buffer.tostring(save.encode(bytes)) == buffer.tostring(encoded))
        local info = save.info(bytes)
        assert(info.region == reference.region and info.variant == reference.variant)
        buffer.writeu8(bytes, 0, 0xab)
        buffer.writeu8(bytes, 65535, 0xcd)
        local reread = save.decode(save.encode(bytes))
        assert(buffer.readu8(reread, 0) == 0xab and buffer.readu8(reread, 65535) == 0xcd)
    end
end
