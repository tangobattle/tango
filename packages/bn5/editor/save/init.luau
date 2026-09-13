--!strict
local codec = require("@bn-common/save/codec")
local copy = require("@editor-common").copy
local t = require("../i18n")
local START, SIZE = 0x100, 0x7c14
local MASK, CHECKSUM = START + 0x1a34, START + 0x29dc
local NAMES: {[string]: {region: string, variant: string}} = {
    ["REXE5TOB 20041104 JP"] = {region = "JP", variant = "Protoman"},
    ["REXE5TOK 20041104 JP"] = {region = "JP", variant = "Colonel"},
    ["REXE5TOB 20041006 US"] = {region = "US", variant = "Protoman"},
    ["REXE5TOK 20041006 US"] = {region = "US", variant = "Colonel"},
}

local function info(bytes: buffer): {region: string, variant: string}
    local found = NAMES[buffer.readstring(bytes, START + 0x29e0, 20)]
    if not found then return tango.fail(t, "unknown-save") end
    return found
end

local function checksum(bytes: buffer): number
    return codec.checksum(bytes, START, SIZE, CHECKSUM) + (if info(bytes).variant == "Protoman" then 0x72 else 0x18)
end

local function decode(input: buffer): buffer
    if buffer.len(input) < START + SIZE then return tango.fail(t, "short-save") end
    local bytes = copy(input)
    codec.mask(bytes, START, SIZE, MASK)
    if buffer.readu32(bytes, START + 0x1a30) ~= 0 then return tango.fail(t, "unsupported-save-shift") end
    if buffer.readu32(bytes, CHECKSUM) ~= checksum(bytes) then return tango.fail(t, "invalid-checksum") end
    return bytes
end

local function encode(input: buffer): buffer
    if buffer.len(input) < START + SIZE then return tango.fail(t, "short-save") end
    local bytes = copy(input)
    buffer.writeu32(bytes, CHECKSUM, checksum(bytes))
    codec.mask(bytes, START, SIZE, MASK)
    return bytes
end

return {START = START, SIZE = SIZE, info = info, decode = decode, encode = encode}
