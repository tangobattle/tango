--!strict
local save = require("./init")
local folders = require("./folders")
local references = require("./fixtures/references")
local NONE = 0xffffffff

local function apply(value: folders.Model, op: string, a: {number}): boolean
    if op == "folder" then return value.set_equipped_folder(a[1] + 1)
    elseif op == "chip" then return value.set_chip(a[1] + 1, a[2] + 1, {id = a[3], code = a[4]})
    elseif op == "clear_chip" then return value.set_chip(a[1] + 1, a[2] + 1, nil)
    elseif op == "regular" then return value.set_regular_index(a[1] + 1, if a[2] == NONE then nil else a[2] + 1)
    elseif op == "pack" then return value.set_pack_count(a[1], a[2] + 1, a[3])
    elseif op == "chip_anticheat" then value.rebuild_chip_anticheat(); return true
    else error("Unknown reference action: " .. op) end
end

local function snapshot(value: folders.Model): {number}
    local values = {value.folder_count(), value.equipped_folder() - 1}
    for folder = 1, 3 do
        table.insert(values, (value.regular_index(folder) or 256) - 1)
        for slot = 1, 30 do
            local chip = value.chip(folder, slot)
            table.insert(values, if chip then chip.id else NONE)
            table.insert(values, if chip then chip.code else NONE)
        end
    end
    for _, id in {0, 1, 320} do
        for variant = 1, 4 do table.insert(values, value.pack_count(id, variant) :: number) end
    end
    return values
end

return function()
    local fixture_size = tango.input_size("test.fixture")
    local name = if fixture_size then buffer.tostring(tango.read_input("test.fixture", 0, fixture_size)) else references[1].name
    for _, fixture in references do
        if fixture.name ~= name then continue end
        local reference = tango.read_file("editor/save/fixtures/" .. fixture.name .. "-folders.bin")
        local cursor = 0
        local function u32(): number
            local value = buffer.readu32(reference, cursor); cursor += 4; return value
        end
        local function text(): string
            local length = u32()
            local value = buffer.readstring(reference, cursor, length); cursor += length; return value
        end
        assert(text() == "bn4-folders-v1" and text() == fixture.name)
        local bytes = save.from_wram(tango.read_file("editor/save/templates/" .. fixture.name .. ".raw"), fixture.info)
        buffer.writeu8(bytes, buffer.len(bytes) - 1, 0xab)
        local expected = buffer.fromstring(buffer.tostring(bytes))
        local value = folders.open(bytes)
        for index = 1, u32() do
            local op = text()
            local arguments: {number} = {}
            for _ = 1, u32() do table.insert(arguments, u32()) end
            local accepted = u32() ~= 0
            local label = fixture.name .. " action " .. tostring(index) .. " " .. op
            assert(apply(value, op, arguments) == accepted, label)
            for _ = 1, u32() do
                local offset, byte = u32(), u32()
                buffer.writeu8(expected, save.START + offset, byte)
            end
            assert(buffer.tostring(bytes) == buffer.tostring(expected), label .. " bytes")
            local size = u32()
            local values = snapshot(value)
            assert(#values == size, label .. " snapshot size")
            for field, actual in values do
                assert(actual == u32(), label .. " field " .. tostring(field))
            end
        end
        local encoded = save.encode(bytes)
        assert(buffer.readu8(encoded, 65535) == 0xab)
        buffer.writeu8(encoded, 65535, 0)
        assert(tango.crc32(encoded) == u32(), fixture.name .. " encoded save")
        assert(cursor == buffer.len(reference))
        local before = buffer.tostring(bytes)
        for _, invalid in {-1, 0.5, 0/0, math.huge, 65536} do
            assert(not value.set_equipped_folder(invalid))
            assert(not value.set_chip(1, invalid, nil) and not value.set_regular_index(1, invalid))
            assert(not value.set_chip(1, 1, {id = invalid, code = 0}))
            assert(not value.set_pack_count(1, 1, invalid))
        end
        assert(buffer.tostring(bytes) == before)
    end
end
