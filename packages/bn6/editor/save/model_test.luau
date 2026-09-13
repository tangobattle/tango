--!strict
local save = require("./init")
local model = require("./model")
local rom = require("../rom")
local stats = require("../navi/stats")
local common = require("@editor-common")
local NONE = 0xffffffff

-- Native reference files retain Rust's zero-based positions. Translate only at
-- this comparison boundary; package consumers use ordinary 1-based indices.
local function apply(value: model.Model, assets: rom.Assets?, op: string, a: {number}): boolean
    if op == "navi" then return value.set_navi(a[1])
    elseif op == "folder" then return value.set_equipped_folder(a[1] + 1)
    elseif op == "chip" then return value.set_chip(a[1] + 1, a[2] + 1, {id = a[3], code = a[4]})
    elseif op == "clear_chip" then return value.set_chip(a[1] + 1, a[2] + 1, nil)
    elseif op == "regular" then return value.set_regular_index(a[1] + 1, if a[2] == NONE then nil else a[2] + 1)
    elseif op == "tags" then return value.set_tag_indexes(a[1] + 1, if a[2] == NONE then nil else {a[2] + 1, a[3] + 1})
    elseif op == "pack" then return value.set_pack_count(a[1], a[2] + 1, a[3])
    elseif op == "chip_anticheat" then value.rebuild_chip_anticheat(); return true
    elseif op == "part" then return value.set_navicust_part(a[1] + 1, if a[2] == NONE then nil
        else {id = a[2], col = a[3], row = a[4], rot = a[5], compressed = a[6] ~= 0})
    elseif op == "clear_grid" then return value.clear_materialized()
    elseif op == "rebuild_grid" then assert(assets); return value.rebuild_materialized(assets)
    elseif op == "card_count" then return value.set_patch_count(a[1])
    elseif op == "card" then return value.set_patch_card(a[1] + 1, {id = a[2], enabled = a[3] ~= 0})
    elseif op == "card_anticheat" then return value.rebuild_patch_anticheat()
    else error("Unknown reference action: " .. op) end
end

local function snapshot(value: model.Model, assets: rom.Assets?): {number}
    local values = {value.navi(), value.folder_count(), value.equipped_folder() - 1}
    for folder = 1, 3 do
        table.insert(values, (value.regular_index(folder) or 256) - 1)
        local tags = value.tag_indexes(folder) or {256, 256}
        table.insert(values, tags[1] - 1); table.insert(values, tags[2] - 1)
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
    if assets then
        local derived = stats.derive(value, assets)
        for _, field in {derived.max_hp, derived.attack, derived.speed, derived.charge, derived.b_power_attack,
            derived.mega_limit, derived.giga_limit, derived.reg_memory, derived.tag_memory} do table.insert(values, field) end
    end
    return values
end

local function compare(reference: buffer, every_snapshot: boolean)
    local cursor = 0
    local function u32(): number
        local value = buffer.readu32(reference, cursor); cursor += 4; return value
    end
    local function text(): string
        local length = u32()
        local value = buffer.readstring(reference, cursor, length); cursor += length; return value
    end
    assert(text() == "tango.bn6-save-reference-v1")
    local name = text()
    local uses_rom = u32() ~= 0
    assert(u32() == save.SIZE)
    local bytes = buffer.create(65536)
    buffer.copy(bytes, save.START, reference, cursor, save.SIZE)
    cursor += save.SIZE
    buffer.writeu8(bytes, 0, 0xab); buffer.writeu8(bytes, 65535, 0xcd)
    local expected = common.copy(bytes)
    local value = model.open(bytes)
    local wram = buffer.create(save.SIZE)
    buffer.copy(wram, 0, bytes, save.START, save.SIZE)
    local assets = if uses_rom then rom.from_inputs(wram) else nil
    assert(not uses_rom or assets ~= nil)
    local count = u32()
    for index = 1, count do
        local op = text()
        local arguments: {number} = {}
        for _ = 1, u32() do table.insert(arguments, u32()) end
        local accepted = u32() ~= 0
        assert(apply(value, assets, op, arguments) == accepted, name .. " action " .. tostring(index) .. " " .. op)
        for _ = 1, u32() do
            local offset, byte = u32(), u32()
            buffer.writeu8(expected, save.START + offset, byte)
        end
        assert(buffer.tostring(bytes) == buffer.tostring(expected), name .. " bytes " .. tostring(index) .. " " .. op)
        local size = u32()
        -- The bundled suite checks every mutation byte and samples the full
        -- getter snapshot, including every capability transition. Supplying
        -- one reference as input checks every snapshot within the same quota.
        if every_snapshot or op == "navi" or index % 16 == 0 or index == count then
            local values = snapshot(value, assets)
            assert(#values == size, name .. " snapshot size " .. tostring(index))
            for field, actual in values do
                local want = u32()
                if actual ~= want then
                    error(name .. " snapshot " .. tostring(index) .. " " .. op .. " field " .. tostring(field)
                        .. ": " .. tostring(actual) .. " ~= " .. tostring(want))
                end
            end
        else
            cursor += size * 4
        end
    end
    assert(cursor == buffer.len(reference))
end

local function boundaries()
    local bytes = buffer.create(65536)
    buffer.copy(bytes, save.START, tango.read_file("editor/save/fixtures/g_jp.raw"))
    local value = model.open(bytes)
    assert(value.set_navi(0))
    -- Partial tags must still conflict with Regular; each folder's raw metadata
    -- is independent. Keep this corruption test outside the normalized oracle.
    local offset = save.stats(bytes)
    buffer.writeu8(bytes, offset + 0x56, 4); buffer.writeu8(bytes, offset + 0x57, 255)
    assert(value.tag_indexes(1) == nil and not value.set_regular_index(1, 5))
    assert(value.set_tag_indexes(1, nil) and value.set_regular_index(1, 5))
    -- Preserve the JP-only trailing gap, including when an anticheat mirror
    -- crosses the end of the shifted interval.
    buffer.fill(bytes, save.START + 0x50fc, 0xa5, 0x40)
    assert(value.set_patch_count(3) and value.rebuild_patch_anticheat())
    for i = 0, 0x3f do assert(buffer.readu8(bytes, save.START + 0x50fc + i) == 0xa5) end
    local original = buffer.tostring(bytes)
    for _, number in {-1, 0.5, 0/0, math.huge, 256} do
        assert(not value.set_navi(number))
        assert(not value.set_equipped_folder(number))
        assert(not value.set_regular_index(1, number))
        assert(not value.set_pack_count(0, 1, number))
        assert(not value.set_patch_count(number))
        assert(not value.set_navicust_part(1, {id = 4, col = number, row = 3, rot = 0, compressed = false}))
    end
    assert(not value.set_patch_count(241))
    assert(not value.set_chip(1, 1, {id = 512, code = 0}))
    assert(not value.set_chip(1, 1, {id = 1, code = 27}))
    -- Zero is never a collection position, including an optional Regular/Tag
    -- selection. None is represented by nil, not a sentinel index.
    assert(value.chip(0, 1) == nil and value.chip(1, 0) == nil)
    assert(not value.set_equipped_folder(0) and not value.set_equipped_folder(4))
    assert(not value.set_chip(0, 1, nil) and not value.set_chip(1, 0, nil) and not value.set_chip(1, 31, nil))
    assert(not value.set_regular_index(1, 0) and not value.set_regular_index(1, 31))
    assert(not value.set_tag_indexes(1, {0, 1}) and not value.set_tag_indexes(1, {1, 31}))
    assert(value.pack_count(1, 0) == nil and value.pack_count(1, 5) == nil)
    assert(not value.set_pack_count(1, 0, 1) and not value.set_pack_count(1, 5, 1))
    assert(value.navicust_part(0) == nil and not value.set_navicust_part(0, nil) and not value.set_navicust_part(26, nil))
    assert(value.patch_card(0) == nil and not value.set_patch_card(0, {id = 1, enabled = true}))
    assert(buffer.tostring(bytes) == original)
    -- The first and final Lua positions address the first and final encoded
    -- records. This checks the boundary against the format, not a self-roundtrip.
    assert(value.set_equipped_folder(3) and value.equipped_folder() == 3)
    assert(buffer.readu8(bytes, offset + 0x2d) == 2)
    assert(value.set_chip(3, 30, {id = 1, code = 26}))
    assert(buffer.readu16(bytes, save.START + 0x2178 + 2 * 60 + 29 * 2) == 1 + 26 * 512)
    assert(value.set_pack_count(1, 4, 42) and buffer.readu8(bytes, save.START + 0x2230 + 12 + 3) == 42)
    assert(value.set_navicust_part(25, {id = 4, col = 3, row = 3, rot = 0, compressed = false}))
    assert(buffer.readu8(bytes, save.START + 0x4150 + 24 * 8) == 4)
    assert(value.set_patch_count(240) and value.set_patch_card(240, {id = 1, enabled = false}))
    assert(buffer.readu8(bytes, save.START + 0x6620 + 239) == 0x81)
end

return {
    test = function()
        local length = tango.input_size("reference.model")
        if length then compare(tango.read_input("reference.model", 0, length), true); return end
        for _, name in {"g_us", "f_us", "g_jp", "f_jp"} do
            compare(tango.read_file("editor/save/fixtures/" .. name .. "-model.bin"), false)
        end
        boundaries()
    end,
}
