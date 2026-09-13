--!strict
local ui = require("@editor-common/ui")

local function accent(image: Raster): {number}?
    local counts: {[number]: number} = {}
    local order: {number} = {}
    for index = 0, image.width * image.height - 1 do
        local offset = index * 4
        if buffer.readu8(image.rgba, offset + 3) < 128 then continue end
        local key = buffer.readu8(image.rgba, offset) * 65536 + buffer.readu8(image.rgba, offset + 1) * 256
            + buffer.readu8(image.rgba, offset + 2)
        if counts[key] == nil then table.insert(order, key) end
        counts[key] = (counts[key] or 0) + 1
    end
    local best, result = 0, nil :: {number}?
    for _, key in order do
        local r, g, b = math.floor(key / 65536), math.floor(key / 256) % 256, key % 256
        local high, low = math.max(r, g, b) / 255, math.min(r, g, b) / 255
        local saturation = if high > 0 then (high - low) / high else 0
        local score = counts[key] * (saturation + 0.05) * high
        if score > best then best = score; result = {r / 255, g / 255, b / 255, 1} end
    end
    return result
end

local function prepare(source: Raster): (Raster, {number}?)
    local image = ui.crop(source, 1, 0, 15, 15)
    return image, accent(image)
end

return {prepare = prepare, accent = accent}
