--!strict
-- Composable presentation helpers. Labels, actions and domain data belong to callers.
local function row(children: {Node}, spacing: number): Node
    return {kind = "row" :: "row", children = children, spacing = spacing, align_y = "center" :: Align, width = "fill" :: Size}
end

local function column(children: {Node}, spacing: number): Node
    return {kind = "column" :: "column", children = children, spacing = spacing, width = "fill" :: Size}
end

local function caption(value: string, danger: boolean): Node
    local color: Color = if danger then "danger" else "muted"
    return {kind = "text" :: "text", text = value, size = "caption" :: TextSize, color = color}
end

local function right(value: string, width: number, size: TextSize, font: Font): Node
    return {kind = "container" :: "container", width = width, align_x = "end" :: Align,
        child = {kind = "text" :: "text", text = value, size = size, font = font}}
end

local function image(value: Raster?, width: number, height: number?): Node
    if not value then return {kind = "space" :: "space", width = width, height = height} end
    return {kind = "image" :: "image", image = value, width = width, height = height, nearest = true}
end

local function crop(value: Raster, x: number, y: number, width: number, height: number): Raster
    assert(x >= 0 and y >= 0 and width > 0 and height > 0
        and x + width <= value.width and y + height <= value.height)
    local pixels = buffer.create(width * height * 4)
    for line = 0, height - 1 do
        buffer.copy(pixels, line * width * 4, value.rgba, ((y + line) * value.width + x) * 4, width * 4)
    end
    return {width = width, height = height, rgba = pixels}
end

local function pane(header: Node, body: Node): Node
    return {kind = "container" :: "container", width = "fill" :: Size, height = "fill" :: Size, tone = "card" :: Tone,
        child = column({header, {kind = "scroll" :: "scroll", width = "fill" :: Size, height = "fill" :: Size, child = body}}, 0)}
end

local function clear_button(id: string, label: string): Node
    local children: {Node} = {
        {kind = "text", text = "\u{e18e}", font = "icons" :: Font, size = "body" :: TextSize},
        {kind = "text", text = label, size = "body" :: TextSize},
    }
    return {kind = "button" :: "button", id = id, tone = "danger" :: Tone, content_padding = {5, 10, 5, 10},
        child = {kind = "row" :: "row", spacing = 8, align_y = "center" :: Align, children = children}}
end

local function remove_button(id: string): Node
    return {kind = "button" :: "button", id = id, text = "\u{e1b2}", font = "icons" :: Font, size = "body" :: TextSize,
        content_padding = {3, 8, 3, 8}}
end

local function toggle(id: string, label: string, selected: boolean, tint: Color, enabled: boolean): Node
    local tone: Tone = if selected then {tint = tint} else "neutral"
    return {kind = "button" :: "button", id = id, text = label, size = "caption" :: TextSize, enabled = enabled,
        tone = tone, content_padding = {4, 8, 4, 8}}
end

local function stripe(accent: Color?): Node
    return {kind = "space" :: "space", width = 6, height = "fill" :: Size, appearance = {background = accent}}
end

local function unavailable(body: Node, tip: Tooltip?): Node
    local wash: Node = {kind = "space" :: "space", width = "fill" :: Size, height = "fill" :: Size,
        appearance = {background = {theme = "background", alpha = 0.6} :: Color}}
    return {kind = "stack" :: "stack", width = "fill" :: Size, children = {body, wash}, tooltip = tip}
end

local function library_header(placeholder: string, filter: string, choices: {Choice}, sort: string,
    sort_label: string, filter_id: string, sort_id: string): Node
    return {kind = "container" :: "container", width = "fill" :: Size, padding = {8, 12, 8, 12}, child = row({
        {kind = "input" :: "input", id = filter_id, placeholder = placeholder, value = filter,
            width = "fill" :: Size, content_padding = {5, 10, 5, 10}},
        caption(sort_label, false),
        {kind = "select" :: "select", id = sort_id, value = sort, choices = choices, content_padding = {5, 10, 5, 10}},
    }, 10)}
end

return {row = row, column = column, caption = caption, right = right, image = image, crop = crop,
    pane = pane, clear_button = clear_button, remove_button = remove_button, toggle = toggle,
    stripe = stripe, unavailable = unavailable, library_header = library_header}
