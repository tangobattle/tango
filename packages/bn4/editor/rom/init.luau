--!strict
local t = require("../i18n")
local gba = require("@gba")
local offsets = require("./offsets")
local charset = require("./charset")
local message = require("./message")
local source = require("@bn-common/rom/source")
local chips = require("@bn-common/rom/chips")
local navicust = require("@bn-common/rom/navicust")
local effects = require("../navi/effects")
local cards = require("../patch_cards/catalog")

export type Chip = chips.Chip
export type Part = navicust.Part
export type Card = cards.Card
export type Assets = {
    definition: offsets.Definition, navicust_command_line: number,
    chip: (number) -> Chip?, chip_is_legal: (number) -> boolean, element_icon: (number) -> Raster?,
    navicust_part: (number) -> Part?, patch_card: (number) -> Card?,
}

local function definition(rom: buffer): offsets.Definition
    if buffer.len(rom) < 0xc0 then return tango.fail(t, "short-rom-header") end
    local found = offsets.definitions[buffer.readstring(rom, 0xac, 4)]
    if not found or buffer.readu8(rom, 0xbc) ~= found.revision then return tango.fail(t, "unsupported-rom") end
    return found
end
local exports = {definition = definition}

local function open(rom: buffer, wram: buffer?, override: offsets.Definition?, custom_characters: {string}?): Assets
    if buffer.len(rom) < 0xc0 then return tango.fail(t, "short-rom-header") end
    local definition = override or exports.definition(rom)
    local characters = custom_characters or (if definition.region == "JP" then charset.JA else charset.EN)
    local mapper = gba.mapper(rom, wram)
    local base = source.open(mapper, characters)
    local reader: source.Source = {region = base.region, indirect = base.indirect,
        text = function(address: number, index: number, resolve: message.Resolve?): string?
            return message.entry(base.indirect(address), index, characters, resolve)
        end,
    }
    local shared = chips.open(reader, definition, {num_chips = 350, num_elements = 13, dark_mask = 0x20,
        element_offset = 7, class_offset = 8, mb_offset = 6,
        resolve_name = function(kind: string, argument: number): string?
            if kind ~= "ereader_name" then return "" end
            local region = mapper.region(0x02000000 + 0x1772 + argument * 0x10)
            return if region then message.parse(region, 0, region.length, characters, nil) else nil
        end,
        resolve_description = function(kind: string, argument: number): string?
            if kind ~= "ereader_description" then return "" end
            local region = mapper.region(0x02000000 + 0x522 + argument * 0x5c)
            return if region then message.parse(region, 0, region.length, characters, nil) else nil
        end,
    })
    return {
        definition = definition, navicust_command_line = 2,
        chip = shared.chip, chip_is_legal = shared.chip_is_legal, element_icon = shared.element_icon,
        -- BN4 does not use the compressed ROM bitmaps; UnderSht's is incorrect.
        navicust_part = navicust.open(reader, definition, {count = 188, bitmap_size = 5,
            colors = {"white", "pink", "yellow", "red", "blue", "green"}, compressed = false,
            effects = effects.for_part, bugs = effects.for_group}),
        patch_card = function(id: number): Card? return cards.get(id, definition.region) end,
    }
end
exports.open = open
exports.from_inputs = function(wram: buffer?): Assets?
    local length = tango.input_size("rom")
    return if length then exports.open(tango.read_input("rom", 0, length), wram, nil) else nil
end
return exports
