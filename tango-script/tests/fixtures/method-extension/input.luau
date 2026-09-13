--!strict
-- A tiny synthetic cartridge keeps this dependency-composition test independent
-- of privately owned ROMs. Game-specific fixture bytes remain in Luau.
local mode: GameMode = {
    patch_rom = function(_input: buffer): buffer
        local bytes = buffer.create(0x50000)
        buffer.writestring(bytes, 0xac, "BR5E")
        buffer.writeu32(bytes, 0x42038, 0x08043000)
        buffer.writeu16(bytes, 0x43000, 4)
        buffer.writeu16(bytes, 0x43002, 5)
        buffer.writestring(bytes, 0x43004, "\xe6\x0b\xe6")
        local chip = 0x21da8 + 0x2c
        buffer.writestring(bytes, chip, "\x01\x1a\xff\xff")
        buffer.writeu8(bytes, chip + 7, 1)
        buffer.writeu8(bytes, chip + 8, 40)
        return bytes
    end,
}
return mode
