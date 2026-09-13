--!strict
local t = require("../i18n")
local gba = require("@gba")
local offsets = require("./offsets")
local charset = require("./charset")
local message = require("@bn-common/rom/message")
local source = require("@bn-common/rom/source")
local chips = require("@bn-common/rom/chips")
local parts = require("./parts")

export type Chip = chips.Chip
export type Assets = {
    definition: offsets.Definition,
    navicust_command_line: number,
    chip: (number) -> Chip?,
    chip_is_legal: (number) -> boolean,
    element_icon: (number) -> Raster?,
    navicust_part: (number) -> parts.Part?,
    patch_card: (number) -> parts.Card?,
    navi: (number) -> parts.Navi?,
}

local function definition(rom: buffer): offsets.Definition
    if buffer.len(rom) < 0xc0 then return tango.fail(t, "short-rom-header") end
    local found = offsets.definitions[buffer.readstring(rom, 0xac, 4)]
    if not found or buffer.readu8(rom, 0xbc) ~= 0 then return tango.fail(t, "unsupported-rom") end
    return found
end

-- Call through the public methods so dependent packages can wrap the reader.
local exports = {definition = definition}

local function open(rom: buffer, wram: buffer?, override: offsets.Definition?, characters: {string}?): Assets
    if buffer.len(rom) < 0xc0 then return tango.fail(t, "short-rom-header") end
    local definition = override or exports.definition(rom)
    local characters = characters or (if definition.region == "JP" then charset.JA else charset.EN)
    local mapper = gba.mapper(rom, wram)
    local extra = parts.open(mapper, definition, characters)
    local shared = chips.open(source.open(mapper, characters, true), definition, {
        num_chips = 368, num_elements = 13, dark_mask = 0x20,
        resolve_name = function(kind: string, argument: number): string?
            if kind ~= "ereader_name" then return "" end
            local region = mapper.region(0x02000000 + 0x1d16 + argument * 0x18)
            return if region then message.parse(region, 0, math.min(region.length, 0x18), characters, nil, true) else nil
        end,
        resolve_description = function(kind: string, argument: number): string?
            if kind ~= "ereader_description" then return "" end
            local region = mapper.region(0x02000000 + 0x1376 + argument * 0x64)
            return if region then message.parse(region, 0, math.min(region.length, 0x64), characters, nil, true) else nil
        end,
    })
    return {
        definition = definition, navicust_command_line = 2,
        chip = shared.chip, chip_is_legal = shared.chip_is_legal, element_icon = shared.element_icon,
        navicust_part = extra.navicust_part, patch_card = extra.patch_card, navi = extra.navi,
    }
end

exports.open = open

local function from_inputs(wram: buffer?): Assets?
    local length = tango.input_size("rom")
    return if length then exports.open(tango.read_input("rom", 0, length), wram, nil) else nil
end

exports.from_inputs = from_inputs
return exports
