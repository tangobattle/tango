--!strict
local ui = require("@editor-common/ui")
local catalog = require("../catalog")

local REG: Color = {255 / 255, 66 / 255, 165 / 255, 1}
local TAG: Color = {41 / 255, 161 / 255, 33 / 255, 1}

local function accent(info: catalog.Chip?): Color?
    if not info then return nil end
    if info.dark then return {74 / 255, 85 / 255, 130 / 255, 1} end
    if info.class == "mega" then return {82 / 255, 132 / 255, 156 / 255, 1} end
    if info.class == "giga" then return {196 / 255, 82 / 255, 132 / 255, 1} end
    return nil
end

local function icon(value: Raster?): Node
    return ui.image(if value then ui.crop(value, 1, 1, 14, 14) else nil, 28, 28)
end

local function code(value: number): string
    return if value == 26 then "*" else string.char(65 + value)
end

local function cells(assets: catalog.Assets, info: catalog.Chip?, chip_code: number): {Node}
    local element = if info then assets.element_icon(info.element) else nil
    return {
        {kind = "text" :: "text", text = if info and info.name then info.name else "???", width = "fill" :: Size},
        if element then icon(element) else {kind = "space" :: "space", width = 28},
        ui.right(code(chip_code), 22, "body", "mono"),
        ui.right(if info and info.attack > 0 then tostring(info.attack) else "", 46, "body", "normal"),
        if assets.chips_have_mb then ui.right(if info and info.mb > 0 then tostring(info.mb) .. "MB" else "", 42, "caption", "normal")
            else {kind = "space" :: "space"},
    }
end

local function inner(assets: catalog.Assets, info: catalog.Chip?, chip_code: number?): Node
    local children: {Node} = {icon(if info then info.icon() else nil)}
    if chip_code == nil then
        table.insert(children, {kind = "text" :: "text", text = "—", color = "muted" :: Color, width = "fill" :: Size} :: Node)
    else
        for _, cell in cells(assets, info, chip_code) do table.insert(children, cell) end
    end
    return ui.row(children, 8)
end

return {accent = accent, icon = icon, code = code, inner = inner, REG = REG, TAG = TAG}
