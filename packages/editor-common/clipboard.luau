--!strict
local function html_escape(value: string): string
    local entities: {[string]: string} = {["&"] = "&amp;", ["<"] = "&lt;", [">"] = "&gt;", ['"'] = "&quot;", ["'"] = "&#39;"}
    return (string.gsub(value, "[&<>\"']", entities))
end

local function png_data_url(image: Raster): string
    return "data:image/png;base64," .. tango.base64_encode(tango.encode_png(image))
end

local function tip(label: string): Tooltip
    return {position = "top" :: TooltipPosition, gap = 4, content = {
        kind = "container" :: "container", padding = 6,
        appearance = {background = "border" :: Color, text_color = "text" :: Color, radius = 4,
            border_width = 1, border_color = {theme = "primary", alpha = 0.45} :: Color},
        child = {kind = "text" :: "text", text = label, size = "caption" :: TextSize},
    }}
end

local function button(id: string, icon: string, label: string, copied: string): Node
    return {kind = "button", id = id, text = icon, font = "icons" :: Font, size = "body" :: TextSize,
        content_padding = {4, 10, 4, 10}, tooltip = tip(label), feedback = {
            child = {kind = "text" :: "text", text = "\u{e219}", font = "icons" :: Font,
                size = "body" :: TextSize, color = "primary" :: Color},
            tooltip = tip(copied),
        }}
end

return {html_escape = html_escape, png_data_url = png_data_url, button = button}
