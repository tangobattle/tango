--!strict
local gba = require("@gba")
local message = require("./message")
export type Source = {
    read region: (number) -> gba.Region?,
    read indirect: (number) -> gba.Region?,
    read text: (number, number, message.Resolve?) -> string?,
}
local function open(mapper: gba.Mapper, characters: {string}, ereader_names: boolean?): Source
    local function indirect(address: number): gba.Region?
        local region = mapper.region(address)
        local pointer = if region then region.u32(0) else nil
        return if pointer then mapper.region(pointer) else nil
    end
    return {region = mapper.region, indirect = indirect,
        text = function(address: number, index: number, resolve: message.Resolve?): string?
            return message.entry(indirect(address), index, characters, resolve, ereader_names)
        end,
    }
end
local function slice(region: gba.Region?, start: number, length: number): buffer?
    return if region then region.slice(start, length) else nil
end
return {open = open, slice = slice}
