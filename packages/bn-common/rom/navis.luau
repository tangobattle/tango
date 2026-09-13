--!strict
local gba = require("@gba")
local source = require("./source")
export type Navi = {id: number, name: string?, emblem: Raster}
export type Offsets = {
    read navi_names_pointer: number, read emblem_icons_pointer: number,
    read emblem_icons_palette_pointer: number, read emblem_icons_palette_offsets_pointer: number,
}
export type Format = {count: number, order: {{number}}, icon_index: (number) -> number?}
local function open(reader: source.Source, definition: Offsets, format: Format): (number) -> Navi?
    local navis: {[number]: Navi} = {}
    return function(id: number): Navi?
        if id < 0 or id >= format.count or id ~= math.floor(id) then return nil end
        local available = false
        for _, row in format.order do
            if table.find(row, id) then available = true; break end
        end
        if not available then return nil end
        if navis[id] then return navis[id] end
        local palette_offsets = reader.indirect(definition.emblem_icons_palette_offsets_pointer)
        local icon_offset = format.icon_index(id)
        local palette_offset = if palette_offsets and palette_offsets.length >= format.count then palette_offsets.u8(id) else nil
        local pixels = if icon_offset then source.slice(reader.indirect(definition.emblem_icons_pointer), icon_offset * 128, 128) else nil
        local palette = if palette_offset then source.slice(reader.indirect(definition.emblem_icons_palette_pointer), palette_offset * 32, 32) else nil
        local result: Navi = {
            id = id, name = reader.text(definition.navi_names_pointer, id + 1, nil),
            emblem = gba.tiles(pixels, palette, 2, 2),
        }
        navis[id] = result
        return result
    end
end
return {open = open}
