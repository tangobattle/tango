--!strict
local source = require("./source")
export type Effect = {kind: string, value: number?}
export type Part = {
    id: number, name: string?, description: string?, color: string?, color_id: number,
    solid: boolean, effect_group: number, bitmap: (boolean) -> {boolean}?,
    effects: {Effect}, bugs: {string},
}
export type Offsets = {read ncp_data: number, read ncp_names_pointer: number, read ncp_descriptions_pointer: number}
export type Format = {
    count: number, bitmap_size: number, effects: (number) -> {Effect}, bugs: (number) -> {string},
    colors: {string}?, compressed: boolean?,
}
local colors = {"white", "yellow", "pink", "red", "blue", "green"}
local function open(reader: source.Source, definition: Offsets, format: Format): (number) -> Part?
    local part_colors = format.colors or colors
    local parts: {[number]: Part} = {}
    local names: {[number]: string} = {}
    local descriptions: {[number]: string} = {}
    return function(id: number): Part?
        if id < 0 or id >= format.count or id ~= math.floor(id) then return nil end
        if parts[id] then return parts[id] end
        local raw = reader.region(definition.ncp_data)
        local start = id * 16
        if not raw or raw.length < start + 16 then return nil end
        local color = raw.u8(start + 3) :: number
        local group = math.floor(id / 4) + 1
        -- Four color variants share their text. Decode that text once.
        local name = names[group] or reader.text(definition.ncp_names_pointer, group, nil)
        local description = descriptions[group] or reader.text(definition.ncp_descriptions_pointer, group, nil)
        names[group], descriptions[group] = name, description
        local bitmaps: {[number]: {boolean}} = {}
        local result: Part = {
            id = id,
            name = name,
            description = description,
            color = part_colors[color], color_id = if part_colors[color] then color else 0,
            solid = raw.u8(start + 1) == 0,
            effect_group = raw.u8(start + 4) :: number,
            effects = format.effects(id),
            bugs = format.bugs(raw.u8(start + 4) :: number),
            bitmap = function(compressed: boolean): {boolean}?
                if compressed and format.compressed == false then return nil end
                local key = if compressed then 2 else 1
                if bitmaps[key] then return table.clone(bitmaps[key]) end
                local pointer = raw.u32(start + (if compressed then 12 else 8)) :: number
                local bytes = reader.region(pointer)
                if not bytes or bytes.length < format.bitmap_size * format.bitmap_size then return nil end
                local cells: {boolean} = {}
                for i = 1, format.bitmap_size * format.bitmap_size do cells[i] = bytes.u8(i - 1) ~= 0 end
                -- Callers still receive independent arrays, while shape
                -- checks and thumbnails reuse the decoded footprint.
                bitmaps[key] = table.clone(cells)
                return cells
            end,
        }
        parts[id] = result
        return result
    end
end
return {open = open, colors = colors}
