--!strict
-- The GBA games share the repeated-byte XOR mask and byte-sum checksum.
-- Addresses are absolute offsets into the caller's buffer, so surrounding
-- SRAM stays intact when a game edits its own save region.
local function mask(bytes: buffer, start: number, size: number, key_offset: number)
    local key = buffer.readu32(bytes, key_offset)
    local byte = bit32.band(key, 255)
    local word = byte * 0x01010101
    local words = math.floor(size / 4) * 4
    for offset = start, start + words - 4, 4 do
        buffer.writeu32(bytes, offset, bit32.bxor(buffer.readu32(bytes, offset), word))
    end
    for offset = start + words, start + size - 1 do
        buffer.writeu8(bytes, offset, bit32.bxor(buffer.readu8(bytes, offset), byte))
    end
    buffer.writeu32(bytes, key_offset, key)
end

local function checksum(bytes: buffer, start: number, size: number, checksum_offset: number): number
    local total = 0
    for offset = start, start + size - 1 do
        if offset < checksum_offset or offset >= checksum_offset + 4 then
            total += buffer.readu8(bytes, offset)
        end
    end
    return total
end

return {mask = mask, checksum = checksum}
