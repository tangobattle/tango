--!strict
local gba = require("@gba")
local message = require("@bn-common/rom/message")
export type Resolve = message.Resolve

local function parse(source: gba.Region, start: number, length: number, charset: {string}, resolve: Resolve?): string?
    if start < 0 or length < 0 or start + length > source.length then return nil end
    local text: {string} = {}
    local cursor, finish = start, math.min(start + length, start + 16384)
    while cursor < finish do
        local value = source.u8(cursor) :: number
        cursor += 1
        local next_byte = if cursor < finish then source.u8(cursor) else nil
        if value < 0xe4 then
            table.insert(text, charset[value + 1] or "")
        elseif value == 0xe4 and next_byte and charset[0xe4 + next_byte + 1] then
            table.insert(text, charset[0xe4 + next_byte + 1])
            cursor += 1
        elseif value == 0xe5 then return table.concat(text)
        elseif value == 0xe8 then table.insert(text, "\n")
        elseif value == 0xff and (next_byte == 0 or next_byte == 1) then
            if cursor + 2 > finish then return nil end
            if resolve then
                local replacement = resolve(if next_byte == 0 then "ereader_name" else "ereader_description", source.u8(cursor + 1) :: number)
                if not replacement then return nil end
                table.insert(text, replacement)
            end
            cursor += 2
        elseif value == 0xe6 then cursor += 1
        elseif value == 0xe7 and next_byte and next_byte >= 1 and next_byte <= 3 then cursor += 1
        elseif value == 0xed and next_byte == 0 then cursor += 3
        elseif value == 0xf0 and next_byte == 0 then cursor += 2
        elseif value == 0xfc and next_byte == 6 then cursor += 1 end
        if cursor > finish then return nil end
    end
    return if length <= 16384 then table.concat(text) else nil
end

local function entry(source: gba.Region?, index: number, charset: {string}, resolve: Resolve?): string?
    if not source or index < 1 or index ~= math.floor(index) then return nil end
    local table_size = source.u16(0)
    if not table_size or table_size % 2 ~= 0 or index > table_size / 2 then return nil end
    local start = source.u16((index - 1) * 2)
    local finish = if index < table_size / 2 then source.u16(index * 2) else source.length
    if not start or not finish or start < table_size or finish < start then return nil end
    return parse(source, start, finish - start, charset, resolve)
end
return {parse = parse, entry = entry}
