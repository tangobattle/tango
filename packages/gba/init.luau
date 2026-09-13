--!strict
-- Cartridge/WRAM mapping and 4bpp tile graphics; decompression uses tango.unlz77.
export type Region = {
    length: number,
    u8: (number) -> number?,
    u16: (number) -> number?,
    u32: (number) -> number?,
    slice: (number, number) -> buffer?,
}
export type Mapper = { region: (number) -> Region? }

local function region(data: buffer, start: number): Region
    local length = buffer.len(data) - start
    local function valid(offset: number, count: number): boolean
        return offset >= 0 and offset == math.floor(offset) and count >= 0
            and count == math.floor(count) and offset + count <= length
    end
    return {
        length = length,
        u8 = function(offset: number): number?
            return if valid(offset, 1) then buffer.readu8(data, start + offset) else nil
        end,
        u16 = function(offset: number): number?
            return if valid(offset, 2) then buffer.readu16(data, start + offset) else nil
        end,
        u32 = function(offset: number): number?
            return if valid(offset, 4) then buffer.readu32(data, start + offset) else nil
        end,
        slice = function(offset: number, count: number): buffer?
            if not valid(offset, count) then return nil end
            local copy = buffer.create(count)
            buffer.copy(copy, 0, data, start + offset, count)
            return copy
        end,
    }
end

local function mapper(rom: buffer, wram: buffer?): Mapper
    local cache: {[number]: buffer} = {}
    return {region = function(address: number): Region?
        if address ~= math.floor(address) then return nil end
        local data: buffer?, offset: number = nil, 0
        if address >= 0x02000000 and address < 0x04000000 then
            data, offset = wram, address - 0x02000000
        elseif address >= 0x08000000 and address < 0x0a000000 then
            data, offset = rom, address - 0x08000000
        elseif address >= 0x88000000 and address < 0x8a000000 then
            local compressed = address - 0x88000000
            if compressed > buffer.len(rom) then return nil end
            data = cache[address]
            if not data then
                data = tango.unlz77(rom, compressed)
                if not data then return nil end
                cache[address] = data
            end
            -- BN's compressed pointer convention prefixes the payload with
            -- four bytes inside the BIOS-compressed stream.
            offset = 4
        else
            return nil
        end
        if not data or offset > buffer.len(data) then return nil end
        return region(data, offset)
    end}
end

local function tiles(raw: buffer?, palette: buffer?, columns: number, rows: number): Raster
    assert(columns >= 1 and rows >= 1 and columns * rows <= 4096)
    assert(columns == math.floor(columns) and rows == math.floor(rows))
    local width, height = columns * 8, rows * 8
    local rgba = buffer.create(width * height * 4)
    if not raw or not palette or buffer.len(raw) < columns * rows * 32 or buffer.len(palette) < 32 then
        return {width = width, height = height, rgba = rgba}
    end
    local colors: {number} = {0}
    for i = 1, 15 do
        local color = buffer.readu16(palette, i * 2)
        local r = math.floor(bit32.band(color, 31) * 255 / 31)
        local g = math.floor(bit32.band(bit32.rshift(color, 5), 31) * 255 / 31)
        local b = math.floor(bit32.band(bit32.rshift(color, 10), 31) * 255 / 31)
        colors[i + 1] = bit32.bor(r, bit32.lshift(g, 8), bit32.lshift(b, 16), 0xff000000)
    end
    for tile = 0, columns * rows - 1 do
        local origin = math.floor(tile / columns) * 8 * width + (tile % columns) * 8
        for y = 0, 7 do
            for pair = 0, 3 do
                local packed = buffer.readu8(raw, tile * 32 + y * 4 + pair)
                local pixel = origin + y * width + pair * 2
                buffer.writeu32(rgba, pixel * 4, colors[bit32.band(packed, 15) + 1])
                buffer.writeu32(rgba, (pixel + 1) * 4, colors[bit32.rshift(packed, 4) + 1])
            end
        end
    end
    return {width = width, height = height, rgba = rgba}
end

return {
    mapper = mapper,
    tiles = tiles,
    test = function()
        local raw = buffer.fromstring("\x10\x0a\x00\x00\x04\x00\x00\x00\x00\x61\x20\x00")
        local m = mapper(raw, buffer.fromstring("WRAM"))
        local r = m.region(0x88000000)
        assert(r and r.length == 6 and r.u8(5) == 97 and r.u8(6) == nil)
        local ram = m.region(0x02000000)
        assert(ram and ram.u32(0) == 0x4d415257 and ram.slice(-1, 1) == nil)
        assert(m.region(0) == nil and m.region(0x08000100) == nil)
        assert(mapper(buffer.fromstring("\x10\x03\x00\x00\x80\x00\x00"), nil).region(0x88000000) == nil)
        assert(mapper(buffer.fromstring("\x10\x03\x00\x00"), nil).region(0x88000000) == nil)
        local pixels, pal = buffer.create(128), buffer.create(32)
        buffer.writeu16(pal, 2, 31)
        buffer.writeu8(pixels, 0, 0x01)
        buffer.writeu8(pixels, 32, 0x11)
        buffer.writeu8(pixels, 64, 0x11)
        local image = tiles(pixels, pal, 2, 2)
        assert(image.width == 16 and image.height == 16)
        assert(buffer.readu32(image.rgba, 0) == 0xff0000ff)
        assert(buffer.readu32(image.rgba, 4) == 0)
        assert(buffer.readu32(image.rgba, 8 * 4) == 0xff0000ff)
        assert(buffer.readu32(image.rgba, 8 * 16 * 4) == 0xff0000ff)
    end,
}
