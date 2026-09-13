--!strict
-- Layout and interaction shared by any package's compact tab strip.
export type Tab = {id: string, label: string, icon: string, errors: {string}?}

local function button(tab: Tab, active: boolean): Node
    local content: {Node} = {
        {kind = "text" :: "text", text = tab.icon, font = "icons" :: Font, size = "body" :: TextSize} :: Node,
        {kind = "text" :: "text", text = tab.label} :: Node,
    }
    local pill: Node = {kind = "button" :: "button", id = tab.id, tone = {tab = active} :: Tone, content_padding = {6, 14, 6, 14},
        child = {kind = "row" :: "row", spacing = 8, align_y = "center" :: Align, children = content}}
    local errors = tab.errors
    if not errors or #errors == 0 then return pill end
    local lines: {Node} = {}
    for _, message in errors do
        local line: {Node} = {
            {kind = "text" :: "text", text = "•", color = "danger" :: Color} :: Node,
            {kind = "text" :: "text", text = message, size = "caption" :: TextSize, width = 360} :: Node,
        }
        table.insert(lines, {kind = "row" :: "row", spacing = 6, align_y = "start" :: Align, children = line} :: Node)
    end
    local pip: Node = {kind = "container" :: "container", width = "fill" :: Size, height = "fill" :: Size, padding = 4,
        align_x = "end" :: Align, align_y = "start" :: Align, child = {
            kind = "space" :: "space", width = 7, height = 7, appearance = {background = "danger" :: Color, radius = 3.5,
                shadow = {color = {theme = "danger", alpha = 0.7} :: Color, blur = 6}},
        }}
    return {kind = "stack" :: "stack", children = {pill, pip}, tooltip = {
        position = "follow_cursor" :: TooltipPosition, gap = 8, content = {kind = "container" :: "container", padding = 8, tone = "tooltip" :: Tone,
            child = {kind = "column" :: "column", spacing = 5, children = lines}},
    }}
end

local function fade(angle: number, visible: boolean): Node
    if not visible then return {kind = "space" :: "space"} end
    local background: Background = {angle = angle, stops = {
        {offset = 0, color = "plate" :: Color},
        {offset = 1, color = {theme = "plate", alpha = 0} :: Color},
    }}
    return {kind = "container" :: "container", width = 24, height = "fill" :: Size, child = {kind = "space" :: "space"},
        appearance = {background = background}}
end

local function view(tabs: {Tab}, selected: string, extras: Node, id: string, offset: number): Node
    local buttons: {Node} = {}
    for _, tab in tabs do table.insert(buttons, button(tab, tab.id == selected)) end
    local scroll: Node = {kind = "scroll" :: "scroll", id = id, on_scroll = true, horizontal = true, scrollbar = "hover" :: "hover", width = "fill" :: Size,
        child = {kind = "row" :: "row", spacing = 2, align_y = "center" :: Align, children = buttons}}
    local shades: {Node} = {fade(math.pi / 2, offset > 0.01), {kind = "space" :: "space", width = "fill" :: Size}, fade(3 * math.pi / 2, offset < 0.99)}
    local strip: {Node} = {
        {kind = "container" :: "container", width = "fill" :: Size, child = {kind = "stack" :: "stack", width = "fill" :: Size, children = {
            scroll, {kind = "row" :: "row", width = "fill" :: Size, height = "fill" :: Size, children = shades} :: Node,
        }}} :: Node,
        {kind = "container" :: "container", height = 13 * 1.3 + 12, align_y = "center" :: Align,
            child = extras} :: Node,
    }
    return {kind = "container" :: "container", width = "fill" :: Size, tone = "card" :: Tone,
        child = {kind = "row" :: "row", width = "fill" :: Size, spacing = 8, align_y = "start" :: Align, padding = {4, 8, 4, 8}, children = strip}}
end

return {button = button, view = view}
