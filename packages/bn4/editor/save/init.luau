--!strict
local codec = require("@bn-common/save/codec")
local t = require("../i18n")
export type Info = {region: "JP" | "US" | "ANY", variant: "RedSun" | "BlueMoon"}

-- The document owns two identity bytes before its unmasked SRAM. BN4 encodes
-- identity in its checksum, which becomes stale during edits. These bytes are
-- removed on encode; no application state or on-disk metadata is needed.
local START, SIZE = 2, 0x73d2
local MASK, SHIFT, NAME, CHECKSUM = 0x1554, 0x1550, 0x2208, 0x21e8
local SHIFT_START, SHIFT_END = 0x2130, 0x5e20

local function info(bytes: buffer): Info
    if buffer.len(bytes) < START + SIZE then return tango.fail(t, "short-save") end
    local region, variant = buffer.readu8(bytes, 0), buffer.readu8(bytes, 1)
    if region < 1 or region > 3 or variant > 1 then return tango.fail(t, "unknown-save") end
    return {region = if region == 1 then "JP" else if region == 2 then "US" else "ANY",
        variant = if variant == 0 then "RedSun" else "BlueMoon"}
end

local function identify(bytes: buffer, shift: number): Info
    if buffer.readstring(bytes, START + shift + NAME, 20) ~= "ROCKMANEXE4 20031022" then
        return tango.fail(t, "unknown-save")
    end
    local first = buffer.readu8(bytes, START)
    local difference = buffer.readu32(bytes, START + shift + CHECKSUM)
        - codec.checksum(bytes, START, SIZE, START + shift + CHECKSUM)
    local region: "JP" | "US" | "ANY" = "US"
    if difference ~= 0x16 and difference ~= 0x22 then
        difference += first
        region = "JP"
    end
    if difference ~= 0x16 and difference ~= 0x22 then return tango.fail(t, "invalid-checksum") end
    return {region = if first == 0 then "ANY" else region, variant = if difference == 0x16 then "RedSun" else "BlueMoon"}
end

local function write_info(bytes: buffer, game: Info)
    buffer.writeu8(bytes, 0, if game.region == "JP" then 1 else if game.region == "US" then 2 else 3)
    buffer.writeu8(bytes, 1, if game.variant == "RedSun" then 0 else 1)
end

local function shift_of(bytes: buffer): number
    local shift = buffer.readu32(bytes, START + SHIFT)
    if shift > 0x1fc or shift % 4 ~= 0 then return tango.fail(t, "unsupported-save-shift") end
    return shift
end

local function checksum(bytes: buffer): number
    local game = info(bytes)
    return codec.checksum(bytes, START, SIZE, START + CHECKSUM)
        + (if game.variant == "RedSun" then 0x16 else 0x22)
        - (if game.region == "JP" then buffer.readu8(bytes, START) else 0)
end

local function normalize(bytes: buffer, shift: number)
    buffer.copy(bytes, START + SHIFT_START, bytes, START + SHIFT_START + shift, SHIFT_END - SHIFT_START)
    buffer.fill(bytes, START + SHIFT_END, 0, shift)
    buffer.writeu32(bytes, START + SHIFT, 0)
    buffer.writeu32(bytes, START + CHECKSUM, checksum(bytes))
end

local function decode(input: buffer): buffer
    if buffer.len(input) < SIZE then return tango.fail(t, "short-save") end
    local bytes = buffer.create(START + buffer.len(input))
    buffer.copy(bytes, START, input)
    codec.mask(bytes, START, SIZE, START + MASK)
    local shift = shift_of(bytes)
    write_info(bytes, identify(bytes, shift))
    normalize(bytes, shift)
    return bytes
end

local function encode(bytes: buffer): buffer
    info(bytes)
    if shift_of(bytes) ~= 0 then return tango.fail(t, "unsupported-save-shift") end
    local output = buffer.create(buffer.len(bytes) - START)
    buffer.copy(output, 0, bytes, START)
    buffer.writeu32(output, CHECKSUM, checksum(bytes))
    codec.mask(output, 0, SIZE, MASK)
    return output
end

local function from_wram(input: buffer, game: Info): buffer
    if buffer.len(input) < SIZE then return tango.fail(t, "short-save") end
    local bytes = buffer.create(START + 65536)
    buffer.copy(bytes, START, input, 0, SIZE)
    write_info(bytes, game)
    normalize(bytes, shift_of(bytes))
    return bytes
end

return {START = START, SIZE = SIZE, info = info, from_wram = from_wram, decode = decode, encode = encode}
