--!strict
local gba = require("@gba")
export type Resolve = (string, number) -> string?

local function parse(source: gba.Region, start: number, length: number, charset: {string}, resolve: Resolve?, ereader_names: boolean?): string?
    if start < 0 or length < 0 or start + length > source.length then return nil end
    local text: {string} = {}
    local cursor, finish = start, math.min(start + length, start + 16384)
    while cursor < finish do
        local value = source.u8(cursor) :: number
        cursor += 1
        if value == 0xe6 then return table.concat(text) end
        local next_byte = if cursor < finish then source.u8(cursor) else nil
        if value < 0xe4 then
            table.insert(text, charset[value + 1] or "")
        elseif (value == 0xe4 or value == 0xe5) and next_byte
            and charset[0xe4 + (value - 0xe4) * 256 + next_byte + 1] then
            table.insert(text, charset[0xe4 + (value - 0xe4) * 256 + next_byte + 1])
            cursor += 1
        elseif value == 0xe9 then
            table.insert(text, "\n")
        elseif value == 0xfa and next_byte == 3 then
            if cursor + 3 > finish then return nil end
            if resolve then table.insert(text, resolve("variable", source.u8(cursor + 2) :: number) or "") end
            cursor += 3
        elseif value == 0xff and (next_byte == 1 or (ereader_names and next_byte == 0)) then
            if cursor + 2 > finish then return nil end
            if resolve then
                local replacement = resolve(if next_byte == 0 then "ereader_name" else "ereader_description", source.u8(cursor + 1) :: number)
                if not replacement then return nil end
                table.insert(text, replacement)
            end
            cursor += 2
        elseif value == 0xe7 then
            cursor += 1
        elseif value == 0xe8 and next_byte and next_byte >= 1 and next_byte <= 6 then
            cursor += if next_byte <= 3 then 1 else 3
        elseif value == 0xee and next_byte == 0 then
            cursor += 3
        elseif value == 0xf1 and next_byte == 0 then
            cursor += 2
        end
        if cursor > finish then return nil end
    end
    return if length <= 16384 then table.concat(text) else nil
end

local function entry(source: gba.Region?, index: number, charset: {string}, resolve: Resolve?, ereader_names: boolean?): string?
    if not source or index < 1 or index ~= math.floor(index) then return nil end
    local table_size = source.u16(0)
    if not table_size or table_size % 2 ~= 0 or index > table_size / 2 then return nil end
    local start = source.u16((index - 1) * 2)
    local finish = if index < table_size / 2 then source.u16(index * 2) else source.length
    if not start or not finish or start < table_size or finish < start then return nil end
    return parse(source, start, finish - start, charset, resolve, ereader_names)
end

return {parse = parse, entry = entry}
