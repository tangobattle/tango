--!strict
local gba = require("@gba")
local source = require("./source")
local message = require("./message")
local t = require("../init").t
export type Chip = {
    id: number, name: string?, description: string?, codes: {number},
    element: number, class: string, mb: number, attack: number, order: number,
    legal: boolean, dark: boolean, icon: () -> Raster, image: () -> Raster,
}
export type Offsets = {
    read chip_data: number, read chip_names_pointers: number, read chip_descriptions_pointers: number,
    read chip_icon_palette_pointer: number, read element_icon_palette_pointer: number,
    read element_icons_pointer: number, read legal_chips: {{number}},
}
export type Format = {
    num_chips: number, num_elements: number, dark_mask: number,
    resolve_name: message.Resolve?, resolve_description: message.Resolve?,
    element_offset: number?, class_offset: number?, mb_offset: number?,
}
export type Assets = {
    chip: (number) -> Chip?, chip_is_legal: (number) -> boolean, element_icon: (number) -> Raster?,
}
local function open(reader: source.Source, definition: Offsets, format: Format): Assets
    local icon_palette = source.slice(reader.indirect(definition.chip_icon_palette_pointer), 0, 32)
    local element_palette = source.slice(reader.indirect(definition.element_icon_palette_pointer), 0, 32)
    local data = reader.region(definition.chip_data)
    if not data or not (data.length >= format.num_chips * 0x2c) then
        return tango.fail(t, "short-chip-table")
    end
    local chips: {[number]: Chip} = {}
    local function chip_is_legal(id: number): boolean
        if id ~= math.floor(id) then return false end
        for _, range in definition.legal_chips do
            if id >= range[1] and id <= range[2] then return true end
        end
        return false
    end
    local function chip(id: number): Chip?
        if id < 0 or id >= format.num_chips or id ~= math.floor(id) then return nil end
        if chips[id] then return chips[id] end
        local base = id * 0x2c
        local codes: {number} = {}
        for i = 0, 3 do
            local value = data.u8(base + i) :: number
            if value <= 26 then table.insert(codes, value) end
        end
        local class_id = data.u8(base + (format.class_offset or 7))
        local class = if class_id == 0 then "standard" elseif class_id == 1 then "mega"
            elseif class_id == 2 then "giga" elseif class_id == 4 then "program_advance" else "none"
        local power = data.u16(base + 0x1a) :: number
        local name = reader.text(definition.chip_names_pointers + math.floor(id / 256) * 4, id % 256 + 1, format.resolve_name)
        local description = reader.text(definition.chip_descriptions_pointers + math.floor(id / 256) * 4, id % 256 + 1, format.resolve_description)
        local icon: Raster?, image: Raster? = nil, nil
        local result: Chip = {
            id = id, name = name, description = description, codes = codes,
            element = data.u8(base + (format.element_offset or 6)) :: number, class = class,
            mb = data.u8(base + (format.mb_offset or 8)) :: number, attack = if power < 1000 then power else 0,
            order = data.u16(base + 0x1c) :: number, legal = chip_is_legal(id),
            dark = bit32.band(data.u8(base + 9) :: number, format.dark_mask) ~= 0,
            icon = function(): Raster
                if not icon then
                    icon = gba.tiles(source.slice(reader.region(data.u32(base + 0x20) :: number), 0, 128), icon_palette, 2, 2)
                end
                return icon :: Raster
            end,
            image = function(): Raster
                if not image then
                    image = gba.tiles(source.slice(reader.region(data.u32(base + 0x24) :: number), 0, 1344),
                        source.slice(reader.region(data.u32(base + 0x28) :: number), 0, 32), 7, 6)
                end
                return image :: Raster
            end,
        }
        chips[id] = result
        return result
    end
    return {
        chip = chip,
        chip_is_legal = chip_is_legal,
        element_icon = function(id: number): Raster?
            if id < 0 or id >= format.num_elements or id ~= math.floor(id) then return nil end
            local pixels = source.slice(reader.indirect(definition.element_icons_pointer), id * 128, 128)
            return if pixels then gba.tiles(pixels, element_palette, 2, 2) else nil
        end,
    }
end
return {open = open}
