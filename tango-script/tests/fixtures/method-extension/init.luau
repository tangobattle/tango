--!strict
local rom = require("@bn6/editor/rom")
local open = rom.open
local calls = 0
local definition = rom.definition
local definitions = 0
rom.definition = function(bytes)
    definitions += 1
    return definition(bytes)
end

-- This is ordinary dependency composition. No host override metadata or
-- special editor constructor participates in applying these replacements.
rom.open = function(bytes, wram, definition, characters): rom.Assets
    calls += 1
    local assets = open(bytes, wram, definition, characters)
    local chip = assets.chip
    assets.chip = function(id: number): rom.Chip?
        local original = chip(id)
        if not original then return nil end
        local result = table.clone(original)
        if id == 1 then result.name = "Replacement" end
        return result
    end
    return assets
end

local editor = table.clone(require("@bn6/editor"))
local create_save = editor.create_save
assert(create_save)
editor.create_save = function(name: string): buffer
    local bytes = editor.decode(create_save(name))
    local model = require("@bn6/editor/save/model").open(bytes)
    -- Give the existing UI and validator a known chip to read through the
    -- wrapped ROM method, with enough copies to produce a named warning.
    for slot = 1, 30 do assert(model.set_chip(model.equipped_folder(), slot, {id = 1, code = 1})) end
    return editor.encode(bytes)
end
editor.test = function()
    assert(calls == 0 and definitions == 0, "replacements must not accumulate across callbacks")
    local assets = rom.from_inputs(nil)
    assert(assets and calls == 1, "from_inputs must dispatch through the replaceable method")
    assert(definitions == 1, "open must dispatch through the replaceable definition method")
    local chip = assets.chip(1)
    assert(chip and chip.name == "Replacement")
    -- Wrapping an individual method leaves graphics and other chip fields alone.
    assert(chip.mb == 40 and chip.class == "mega" and chip.codes[1] == 1)
    local pixels = chip.icon()
    assert(pixels.width == 16 and pixels.height == 16)
    local size = tango.input_size("rom")
    assert(size)
    local original = open(tango.read_input("rom", 0, size), nil, nil).chip(1)
    assert(original and original.name == "A", "the wrapper must not mutate the original asset")

    local mode = table.clone(require("@bn6/gamemode/single"))
    mode.patch_rom = function(bytes: buffer): buffer return bytes end
    local patch_rom = mode.patch_rom
    assert(patch_rom and patch_rom(buffer.create(0)))
    assert(require("@bn6/gamemode/single").patch_rom == nil)

    local base = require("@bn6/telemetry")
    local telemetry = table.clone(base)
    local poll = telemetry.poll
    telemetry.poll = function(state: buffer, context: TelemetryContext, memory: Memory): TelemetryFrame
        return poll(state, context, memory)
    end
    assert(telemetry.poll ~= base.poll)
end
return editor
