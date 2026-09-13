--!strict
local save = require("./init")
local model = require("./model")
local materialize = require("@bn-common/navicust/materialize")
local common = require("@editor-common")
local stats = require("../navi/stats")
local effects = require("../navi/effects")
local NONE = 0xffffffff

-- Synthetic assets match the native fixture generator. No game ROM is needed.
local assets = {
    navicust_command_line = 2,
    patch_card = function(_id: number) return nil end,
    num_chips = 368,
    chip_class = function(id: number): string?
        return ({"standard", "mega", "giga", "program_advance", "none"})[id % 5 + 1]
    end,
    navicust_part = function(id: number): (materialize.Part & {effects: {effects.Effect}})?
        if id <= 0 or id >= 192 then return nil end
        return {effects = effects.for_part(id), color_id = (id - 1) % 6 + 1, bitmap = function(compressed: boolean): {boolean}?
            if compressed and id % 2 == 0 then return nil end
            local bitmap: {boolean} = {}
            for index = 0, 24 do
                local x, y = index % 5, math.floor(index / 5)
                bitmap[index + 1] = (index + id) % 7 == 0 or (x == 2 and (y == 2 or (not compressed and y == 1)))
            end
            return bitmap
        end}
    end,
}

-- Native positions are zero-based; convert only at the oracle boundary.
local function apply(value: model.Model, op: string, a: {number}): boolean
    if op == "navi" then return value.set_navi(a[1])
    elseif op == "folder" then return value.set_equipped_folder(a[1] + 1)
    elseif op == "chip" then return value.set_chip(a[1] + 1, a[2] + 1, {id = a[3], code = a[4]})
    elseif op == "clear_chip" then return value.set_chip(a[1] + 1, a[2] + 1, nil)
    elseif op == "regular" then return value.set_regular_index(a[1] + 1, if a[2] == NONE then nil else a[2] + 1)
    elseif op == "pack" then return value.set_pack_count(a[1], a[2] + 1, a[3])
    elseif op == "chip_anticheat" then value.rebuild_chip_anticheat(); return true
    elseif op == "part" then return value.set_navicust_part(a[1] + 1, if a[2] == NONE then nil
        else {id = a[2], col = a[3], row = a[4], rot = a[5], compressed = a[6] ~= 0})
    elseif op == "clear_grid" then return value.clear_materialized()
    elseif op == "rebuild_grid" then return value.rebuild_materialized(assets)
    elseif op == "card_count" then return value.set_patch_count(a[1])
    elseif op == "card" then return value.set_patch_card(a[1] + 1, {id = a[2], enabled = a[3] ~= 0})
    elseif op == "card_anticheat" then return value.rebuild_patch_anticheat()
    elseif op == "karma" then return value.set_karma(a[1])
    elseif op == "dark_hp" then return value.set_dark_hp_losses(a[1])
    elseif op == "uses" then return value.set_chip_use_count(a[1], a[2], a[3] ~= 0)
    elseif op == "clear_abd" then value.clear_auto_battle_data(); return true
    elseif op == "rebuild_abd" then value.rebuild_auto_battle_data(assets); return true
    else error("Unknown reference action: " .. op) end
end

local function snapshot(value: model.Model): {number}
    local values = {value.navi(), value.folder_count(), value.equipped_folder() - 1}
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
    table.insert(values, if value.has_navicust() then 1 else 0)
    if value.has_navicust() then
        for slot = 1, 25 do
            local part = value.navicust_part(slot)
            local fields: {number} = if part then {part.id, part.col, part.row, part.rot, if part.compressed then 1 else 0}
                else {NONE, NONE, NONE, NONE, NONE}
            for _, field in fields do table.insert(values, field) end
        end
        for _, cell in value.materialized() :: {number} do table.insert(values, cell) end
        for _, color in value.color_bar() :: {number} do table.insert(values, color) end
    end
    local count = value.patch_count()
    table.insert(values, count or NONE)
    if count then
        for slot = 1, count do
            local card = value.patch_card(slot)
            assert(card)
            table.insert(values, card.id); table.insert(values, if card.enabled then 1 else 0)
        end
    end
    table.insert(values, value.karma()); table.insert(values, value.dark_hp_losses())
    local ids = {}
    for id = 0, 19 do table.insert(ids, id) end
    table.insert(ids, 366); table.insert(ids, 367)
    for _, id in ids do
        table.insert(values, value.chip_use_count(id, false) :: number)
        table.insert(values, value.chip_use_count(id, true) :: number)
    end
    for _, chip in value.auto_battle_data() do table.insert(values, chip.id or NONE) end
    local derived = stats.derive(value, assets)
    for _, field in {derived.max_hp, derived.mega_limit or NONE, derived.giga_limit or NONE,
        derived.dark_limit or NONE, derived.reg_memory or NONE} do table.insert(values, field) end
    for id = 0, 4 do
        local limit = stats.max_copies(value, {class = assets.chip_class(id) :: string, dark = false})
        table.insert(values, math.min(limit, NONE))
    end
    return values
end

local function compare(name: string)
    local reference = tango.read_file("editor/save/fixtures/" .. name .. "-model.bin")
    local cursor = 0
    local function u32(): number
        local value = buffer.readu32(reference, cursor); cursor += 4; return value
    end
    local function text(): string
        local length = u32()
        local value = buffer.readstring(reference, cursor, length); cursor += length; return value
    end
    assert(text() == "bn5-model-v1" and text() == name)
    local bytes = buffer.create(65536)
    buffer.copy(bytes, save.START, tango.read_file("editor/save/fixtures/" .. name .. ".raw"))
    buffer.writeu8(bytes, 0, 0xab); buffer.writeu8(bytes, 65535, 0xcd)
    local expected = common.copy(bytes)
    local value = model.open(bytes)
    for index = 1, u32() do
        local op = text()
        local arguments: {number} = {}
        for _ = 1, u32() do table.insert(arguments, u32()) end
        local accepted = u32() ~= 0
        local label = name .. " action " .. tostring(index) .. " " .. op
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
            local want = u32()
            assert(actual == want, label .. " field " .. tostring(field) .. ": " .. tostring(actual) .. " ~= " .. tostring(want))
        end
    end
    assert(cursor == buffer.len(reference))
end

local function boundaries()
    local bytes = buffer.create(65536)
    buffer.copy(bytes, save.START, tango.read_file("editor/save/fixtures/light_protoman_us.raw"))
    local value = model.open(bytes)
    assert(value.set_navi(0))
    local before = buffer.tostring(bytes)
    for _, invalid in {-1, 0.5, 0/0, math.huge, 65536} do
        assert(not value.set_navi(invalid))
        assert(not value.set_karma(invalid) and not value.set_dark_hp_losses(invalid))
        assert(not value.set_chip_use_count(1, invalid) and not value.set_chip_use_count(invalid, 1))
        assert(not value.set_patch_count(invalid))
        assert(not value.set_pack_count(1, 1, invalid))
        assert(not value.set_navicust_part(1, {id = 1, col = invalid, row = 0, rot = 0, compressed = false}))
    end
    assert(not value.set_navi(13) and not value.set_equipped_folder(0))
    assert(value.chip(0, 1) == nil and value.chip(1, 0) == nil)
    assert(not value.set_chip(1, 0, nil) and not value.set_chip(1, 1, {id = 512, code = 0}))
    assert(not value.set_chip(1, 1, {id = 1, code = 27}) and not value.set_regular_index(1, 0))
    assert(not value.set_pack_count(1, 0, 1) and not value.set_pack_count(1, 5, 1))
    assert(not value.set_navicust_part(0, nil) and not value.set_patch_count(256))
    assert(not value.set_patch_card(0, {id = 1, enabled = true}))
    assert(buffer.tostring(bytes) == before)
end

return function()
    for _, reference in require("./fixtures/references") do compare(reference.name) end
    boundaries()
end
