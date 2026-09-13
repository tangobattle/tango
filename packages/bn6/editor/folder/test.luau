--!strict
local save = require("../save")
local model = require("../save/model")
local rom = require("../rom")
local editor = require("./init")
local common = require("@editor-common")
local NONE = 0xffffffff

-- Translate the native fixture format at its boundary. All editor positions
-- and operations inside the package are 1-based.
local function action(op: string, a: {number}): editor.Action
    if op == "clear" then return {kind = "clear"}
    elseif op == "add" then return {kind = "add", chip = {id = a[1], code = a[2]}}
    elseif op == "remove" then return {kind = "remove", slot = a[1] + 1}
    elseif op == "move" then return {kind = "move", from = a[1] + 1, to = a[2] + 1}
    elseif op == "regular" then return {kind = "regular", slot = a[1] + 1}
    elseif op == "tags" then return {kind = "tags", pair = if a[1] == NONE then nil else {a[1] + 1, a[2] + 1}}
    elseif op == "set" then return {kind = "set", slot = a[1] + 1, chip = if a[2] == NONE then nil else {id = a[2], code = a[3]}}
    else error("Unknown folder operation: " .. op) end
end

local function test()
    local length = tango.input_size("reference.folder")
    assert(length, "Native folder reference required")
    local reference = tango.read_input("reference.folder", 0, length)
    local cursor = 0
    local function u32(): number
        local value = buffer.readu32(reference, cursor); cursor += 4; return value
    end
    local function text(): string
        local length = u32()
        local value = buffer.readstring(reference, cursor, length); cursor += length; return value
    end
    assert(text() == "tango.bn6-folder-reference-v1")
    local name = text()
    assert(u32() == save.SIZE)
    local bytes, wram = buffer.create(65536), buffer.create(save.SIZE)
    buffer.copy(wram, 0, reference, cursor, save.SIZE); cursor += save.SIZE
    buffer.copy(bytes, save.START, wram)
    buffer.writeu8(bytes, 0, 0xab); buffer.writeu8(bytes, 65535, 0xcd)
    local expected = common.copy(bytes)
    local assets = rom.from_inputs(wram)
    assert(assets)
    local value = model.open(bytes)
    for index = 1, u32() do
        local op = text()
        local arguments: {number} = {}
        for _ = 1, u32() do table.insert(arguments, u32()) end
        editor.apply(value, assets, value.equipped_folder(), action(op, arguments))
        for _ = 1, u32() do
            local offset, byte = u32(), u32()
            buffer.writeu8(expected, save.START + offset, byte)
        end
        assert(buffer.tostring(bytes) == buffer.tostring(expected), name .. " operation " .. tostring(index) .. " " .. op)
    end
    assert(cursor == buffer.len(reference))
end

return {test = test}
