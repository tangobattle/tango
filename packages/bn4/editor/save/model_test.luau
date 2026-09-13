--!strict
local save = require("./init")
local navicust = require("./navicust")
local patch_cards = require("./patch_cards")
local auto_battle_data = require("./auto_battle_data")
local materialize = require("@bn-common/navicust/materialize")
local references = require("./fixtures/references")
local model = require("./model")
local stats = require("../navi/stats")
local effects = require("../navi/effects")
local catalog = require("../patch_cards/catalog")
local rules = require("../patch_cards/rules")
local NONE = 0xffffffff

-- Synthetic assets match the native reference generator, including compressed
-- fallback behavior. Real BN4 ROM assets expose no compressed bitmaps.
local assets = {
    num_chips = 350,
    chip_class = function(id: number): string?
        return ({"standard", "mega", "giga", "program_advance", "none"})[id % 5 + 1]
    end,
    navicust_part = function(id: number): materialize.Part?
        if id <= 0 or id >= 188 then return nil end
        return {color_id = ({1, 3, 2, 4, 5, 6})[(id - 1) % 6 + 1], bitmap = function(compressed: boolean): {boolean}?
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
local stat_assets = {
    navicust_command_line = 2,
    navicust_part = function(id: number): {effects: {effects.Effect}}?
        return if id > 0 and id < 188 then {effects = effects.for_part(id)} else nil
    end,
    patch_card = function(id: number): catalog.Card? return catalog.get(id, "US") end,
}

local function apply(bytes: buffer, customization: navicust.Model, cards: patch_cards.Model,
    data: auto_battle_data.Model, op: string, a: {number}): boolean
    if op == "part" then return customization.set_navicust_part(a[1] + 1, if a[2] == NONE then nil
        else {id = a[2], col = a[3], row = a[4], rot = a[5], compressed = a[6] ~= 0})
    elseif op == "clear_grid" then return customization.clear_materialized()
    elseif op == "rebuild_grid" then return customization.rebuild_materialized(assets)
    elseif op == "card" then return cards.set_patch_card(a[1] + 1, if a[2] == NONE then nil else {id = a[2], enabled = a[3] ~= 0})
    elseif op == "card_anticheat" then cards.rebuild_patch_anticheat(); return true
    elseif op == "uses" then return data.set_chip_use_count(a[1], a[2], a[3] ~= 0)
    elseif op == "clear_abd" then data.clear_auto_battle_data(); return true
    elseif op == "rebuild_abd" then data.rebuild_auto_battle_data(assets); return true
    elseif op == "raw" then
        -- Simulate an externally stored malformed binding/grid, then reopen as
        -- the native reference does. This operation is only in the test adapter.
        buffer.writeu8(bytes, save.START + a[1], a[2])
        local raw = buffer.create(save.SIZE)
        buffer.copy(raw, 0, bytes, save.START, save.SIZE)
        buffer.copy(bytes, 0, save.from_wram(raw, save.info(bytes)), 0, save.START + save.SIZE)
        return true
    else error("Unknown reference action: " .. op) end
end

local function snapshot(customization: navicust.Model, cards: patch_cards.Model, data: auto_battle_data.Model, value: model.Model): {number}
    local values: {number} = {}
    for slot = 1, 25 do
        local part = customization.navicust_part(slot)
        local fields: {number} = if part then {part.id, part.col, part.row, part.rot, if part.compressed then 1 else 0}
            else {NONE, NONE, NONE, NONE, NONE}
        for _, field in fields do table.insert(values, field) end
    end
    for _, cell in customization.materialized() :: {number} do table.insert(values, cell) end
    for _, color in customization.color_bar() :: {number} do table.insert(values, color) end
    assert(cards.patch_count() == 6)
    for slot = 1, 6 do
        local card = cards.patch_card(slot)
        table.insert(values, if card then card.id else NONE)
        table.insert(values, if card then (if card.enabled then 1 else 0) else NONE)
    end
    local ids = {}
    for id = 0, 19 do table.insert(ids, id) end
    table.insert(ids, 348); table.insert(ids, 349)
    for _, id in ids do
        table.insert(values, data.chip_use_count(id, false) :: number)
        table.insert(values, data.chip_use_count(id, true) :: number)
    end
    for _, chip in data.auto_battle_data() do table.insert(values, chip.id or NONE) end
    local derived = stats.derive(value, stat_assets)
    for _, field in {derived.max_hp, derived.mega_limit, derived.giga_limit, derived.reg_memory} do table.insert(values, field) end
    local findings = rules.violations(value, stat_assets)
    table.insert(values, #findings)
    for _, finding in findings do
        for _, field in {finding.slot, finding.id, if finding.enabled then 1 else 0, finding.expected_slot or 0} do
            table.insert(values, field)
        end
    end
    return values
end

return function()
    local fixture_size = tango.input_size("test.fixture")
    local name = if fixture_size then buffer.tostring(tango.read_input("test.fixture", 0, fixture_size)) else references[1].name
    for _, fixture in references do
        if fixture.name ~= name then continue end
        local reference = tango.read_file("editor/save/fixtures/" .. fixture.name .. "-model.bin")
        local cursor = 0
        local function u32(): number
            local value = buffer.readu32(reference, cursor); cursor += 4; return value
        end
        local function text(): string
            local length = u32()
            local value = buffer.readstring(reference, cursor, length); cursor += length; return value
        end
        assert(text() == "bn4-model-v2" and text() == fixture.name)
        local bytes = save.from_wram(tango.read_file("editor/save/templates/" .. fixture.name .. ".raw"), fixture.info)
        buffer.writeu8(bytes, buffer.len(bytes) - 1, 0xab)
        local expected = buffer.fromstring(buffer.tostring(bytes))
        local customization = navicust.open(bytes)
        local cards = patch_cards.open(bytes)
        local data = auto_battle_data.open(bytes)
        local value = model.open(bytes)
        for index = 1, u32() do
            local op = text()
            local arguments: {number} = {}
            for _ = 1, u32() do table.insert(arguments, u32()) end
            local accepted = u32() ~= 0
            local label = fixture.name .. " action " .. tostring(index) .. " " .. op
            assert(apply(bytes, customization, cards, data, op, arguments) == accepted, label)
            for _ = 1, u32() do
                local offset, byte = u32(), u32()
                buffer.writeu8(expected, save.START + offset, byte)
            end
            if buffer.tostring(bytes) ~= buffer.tostring(expected) then
                for offset = 0, buffer.len(bytes) - 1 do
                    assert(buffer.readu8(bytes, offset) == buffer.readu8(expected, offset), label .. " byte " .. tostring(offset - save.START))
                end
            end
            local size = u32()
            local values = snapshot(customization, cards, data, value)
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
            assert(not customization.set_navicust_part(invalid, nil))
            assert(not customization.set_navicust_part(1, {id = 1, col = invalid, row = 0, rot = 0, compressed = false}))
            assert(not cards.set_patch_card(invalid, nil) and not cards.set_patch_card(1, {id = invalid, enabled = true}))
            assert(not data.set_chip_use_count(invalid, 1) and not data.set_chip_use_count(1, invalid))
        end
        assert(buffer.tostring(bytes) == before)
    end
end
