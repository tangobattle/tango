--!strict
local build = require("./init")
local save = require("../save")
local model = require("../save/model")
local rom = require("../rom")
local parts = require("../rom/parts")
local rules = require("@bn-common/folder/rules")

local function lines(report: build.Report): string
    local rows: {string} = {}
    local function row(values: {string | number})
        local strings: {string} = {}
        for _, value in values do table.insert(strings, tostring(value)) end
        table.insert(rows, table.concat(strings, "\t"))
    end
    for _, finding in report.folder do
        if finding.kind == "folder_not_full" then row({"folder", finding.used, finding.required})
        else
            local issue = finding.issue
            local used, limit = 0, 0
            if issue.kind ~= "chip_illegal_for_game" and issue.kind ~= "chip_code_unavailable" then
                local details = issue :: rules.LimitIssue
                used, limit = details.used, details.limit
            end
            row({"chip", finding.slot - 1, finding.id, finding.code, issue.kind, used, limit})
        end
    end
    for _, finding in report.patch_cards do row({"patch", finding.slot - 1, finding.id, finding.mb, finding.used, finding.limit}) end
    for _, finding in report.navicust_parts do row({"part", finding.slot - 1, finding.id, finding.col, finding.row}) end
    if report.navicust_mismatch and #report.navicust_parts == 0 then row({"materialization"}) end
    return table.concat(rows, "\n")
end

local function compare(reference: buffer)
    local cursor = 0
    local function u32(): number
        local value = buffer.readu32(reference, cursor); cursor += 4; return value
    end
    local function text(): string
        local length = u32()
        local value = buffer.readstring(reference, cursor, length); cursor += length; return value
    end
    assert(text() == "tango.bn6-build-reference-v1")
    local name = text()
    assert(text() == tango.locale(), "Reference locale differs")
    assert(u32() == save.SIZE)
    local bytes, wram = buffer.create(65536), buffer.create(save.SIZE)
    buffer.copy(wram, 0, reference, cursor, save.SIZE); cursor += save.SIZE
    buffer.copy(bytes, save.START, wram)
    buffer.writeu8(bytes, 0, 0xab); buffer.writeu8(bytes, 65535, 0xcd)
    local before = buffer.tostring(bytes)
    local assets = rom.from_inputs(wram)
    assert(assets, "Reference requires a ROM")
    local report = build.inspect(bytes, assets)
    local expected = text()
    local actual = lines(report)
    if actual ~= expected then error(name .. " findings:\n" .. actual .. "\nexpected:\n" .. expected) end
    local warnings = build.warnings(report, assets)
    assert(#warnings == u32(), name .. " warning count")
    for index, warning in warnings do
        local expected_warning = text()
        if warning ~= expected_warning then error(name .. " warning " .. tostring(index) .. ": " .. warning .. " ~= " .. expected_warning) end
    end
    assert(cursor == buffer.len(reference))
    assert(buffer.tostring(bytes) == before, "Validation mutated the save")
end

local function synthetic()
    local bytes = buffer.create(65536)
    buffer.copy(bytes, save.START, tango.read_file("editor/save/fixtures/g_jp.raw"))
    local value = model.open(bytes)
    assert(value.set_navi(0))
    for slot = 1, 25 do assert(value.set_navicust_part(slot, nil)) end
    assert(value.clear_materialized())
    local source = buffer.create(0x50000)
    buffer.writestring(source, 0xac, "BR5J")
    local assets = rom.open(source, nil, nil)
    assets.patch_card = function(id: number): parts.Card?
        local mb = if id == 1 then 40 elseif id == 2 then 70 elseif id == 3 then 10 else nil
        return if mb then {id = id, name = "Card", mb = mb, effects = {}} else nil
    end
    assert(value.set_patch_count(5))
    for slot, id in {1, 2, 1, 0, 3} do assert(value.set_patch_card(slot, {id = id, enabled = slot ~= 2})) end
    local before = buffer.tostring(bytes)
    local report = build.inspect(bytes, assets)
    assert(report.patch_mb == 90 and #report.patch_cards == 1)
    assert(report.patch_cards[1].slot == 5 and report.patch_cards[1].used == 90 and report.patch_cards[1].mb == 10)
    assert(not report.navicust_mismatch and #report.navicust_parts == 0)
    assert(buffer.tostring(bytes) == before)
    -- A phantom materialized slot still invalidates the build without
    -- inventing a real part to attach the warning to.
    buffer.writeu8(bytes, save.START + 0x410c, 250)
    report = build.inspect(bytes, assets)
    assert(report.navicust_mismatch and #report.navicust_parts == 0)
    assert(value.set_navi(1))
    report = build.inspect(bytes, assets)
    assert(report.patch_mb == nil and #report.patch_cards == 0 and not report.navicust_mismatch)
end

return {test = function()
    local length = tango.input_size("reference.build")
    if length then compare(tango.read_input("reference.build", 0, length)) else synthetic() end
end}
