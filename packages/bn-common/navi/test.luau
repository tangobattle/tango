--!strict
local emblem = require("./emblem")

local function test()
    local image: Raster = {width = 16, height = 16, rgba = buffer.create(16 * 16 * 4)}
    local function pixel(x: number, y: number, r: number, g: number, b: number, a: number)
        local offset = (y * image.width + x) * 4
        buffer.writeu8(image.rgba, offset, r); buffer.writeu8(image.rgba, offset + 1, g)
        buffer.writeu8(image.rgba, offset + 2, b); buffer.writeu8(image.rgba, offset + 3, a)
    end
    assert(emblem.accent(image) == nil)
    pixel(1, 0, 0, 0, 0, 255)
    assert(emblem.accent(image) == nil) -- zero brightness
    pixel(1, 0, 255, 255, 255, 255)
    assert((emblem.accent(image) :: {number})[1] == 1)
    -- Fully saturated colors can outweigh more common neutral pixels.
    for x = 2, 10 do pixel(x, 0, 128, 128, 128, 255) end
    pixel(1, 1, 255, 0, 0, 128)
    pixel(2, 1, 0, 255, 0, 127) -- below the opaque-pixel threshold
    -- The left column and final row are outside the native emblem crop.
    for y = 0, 15 do pixel(0, y, 0, 0, 255, 255) end
    for x = 0, 15 do pixel(x, 15, 0, 0, 255, 255) end
    local cropped, accent = emblem.prepare(image)
    assert(cropped.width == 15 and cropped.height == 15 and buffer.len(cropped.rgba) == 900)
    assert(buffer.readu32(cropped.rgba, 0) == 0xffffffff)
    assert(accent and accent[1] == 1 and accent[2] == 0 and accent[3] == 0 and accent[4] == 1)
    -- Equal scores use the first pixel's color, independent of hash order.
    pixel(2, 1, 0, 255, 0, 128)
    local _, tied = emblem.prepare(image)
    assert(tied and tied[1] == 1 and tied[2] == 0)
end

return {test = test}
