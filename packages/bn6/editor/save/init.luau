--!strict
local t = require("../i18n")
-- Work in the original regional SRAM layout, preserving unrelated bytes.
local START = 0x100
local SIZE = 0x6710
local MASK = START + 0x1064
local CHECKSUM = START + 0x1c6c
local NAMES: {[string]: {region: string, variant: string}} = {
    ["REXE6 G 20050924a JP"] = {region = "JP", variant = "Gregar"},
    ["REXE6 F 20050924a JP"] = {region = "JP", variant = "Falzar"},
    ["REXE6 G 20060110a US"] = {region = "US", variant = "Gregar"},
    ["REXE6 F 20060110a US"] = {region = "US", variant = "Falzar"},
}

local codec = require("@bn-common/save/codec")
local common = require("@editor-common")
local copy = common.copy

local function info(bytes: buffer): {region: string, variant: string}
    local name = buffer.readstring(bytes, START + 0x1c70, 20)
    local found = NAMES[name]
    if not found then return tango.fail(t, "unknown-save") end
    return found
end

local function checksum(bytes: buffer): number
    return codec.checksum(bytes, START, SIZE, CHECKSUM) + (if info(bytes).variant == "Gregar" then 0x72 else 0x18)
end

local function mask(bytes: buffer)
    codec.mask(bytes, START, SIZE, MASK)
end

local function decode(input: buffer): buffer
    if buffer.len(input) < START + SIZE then return tango.fail(t, "short-save") end
    local bytes = copy(input)
    mask(bytes)
    if buffer.readu32(bytes, START + 0x1060) ~= 0 then return tango.fail(t, "unsupported-save-shift") end
    if buffer.readu32(bytes, CHECKSUM) ~= checksum(bytes) then return tango.fail(t, "invalid-checksum") end
    return bytes
end

local function encode(input: buffer): buffer
    local bytes = copy(input)
    buffer.writeu32(bytes, CHECKSUM, checksum(bytes))
    mask(bytes)
    return bytes
end

local function stats(bytes: buffer): number
    local shift = if info(bytes).region == "JP" then 0x40 else 0
    local operative = bit32.band(buffer.readu8(bytes, START + 0x1cb4), 0x10) ~= 0
        and buffer.readu8(bytes, START + 0x1b81) ~= 0
    return START + 0x47cc - shift + (if operative then 0x64 else 0)
end

local function folders(bytes: buffer): number
    return buffer.readu8(bytes, START + 0x1c09)
end

local function chip_offset(folder: number, slot: number): number
    return START + 0x2178 + (folder - 1) * 60 + (slot - 1) * 2
end

local function rebuild_anticheat(bytes: buffer)
    local game = info(bytes)
    local shift = if game.region == "JP" then 0x40 else 0
    local key = if game.variant == "Gregar" then 0x17 else 0x81
    for offset = 0, 0x1ff do
        buffer.writeu8(bytes, START + 0x4c20 - shift + offset,
            bit32.bxor(buffer.readu8(bytes, START + 0x08a0 + offset), key))
    end
end

local function validate(bytes: buffer): {string}
    local errors: {string} = {}
    if folders(bytes) < 1 or folders(bytes) > 3 then
        return {t("invalid-folder-count")}
    end
    for folder = 1, folders(bytes) do
        for slot = 1, 30 do
            local chip = buffer.readu16(bytes, chip_offset(folder, slot))
            local id, code = bit32.band(chip, 0x1ff), bit32.rshift(chip, 9)
            if id < 1 or id > 320 or code > 26 then
                table.insert(errors, t("invalid-slot", {folder = folder, slot = slot}))
            end
        end
    end
    return errors
end


return {
    START = START, SIZE = SIZE, info = info, decode = decode, encode = encode,
    stats = stats, folders = folders, chip_offset = chip_offset,
    rebuild_anticheat = rebuild_anticheat, validate = validate,
}
