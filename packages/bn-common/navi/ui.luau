--!strict
local ui = require("@editor-common/ui")
export type Entry = {id: number, name: string?, emblem: Raster?, accent: {number}?}
export type Buster = {attack: number, speed: number, charge: number}
export type Identity = {navi: Entry?, hp: number?, buster: Buster?}
local CELL_WIDTH, CELL_PADDING, CELL_GAP = 88, 4, 14

local function stat(label: string, value: number): Node
    return {kind = "row", spacing = 5, align_y = "end" :: Align, children = {
        {kind = "text" :: "text", text = label, size = "caption" :: TextSize, color = "muted" :: Color, wrapping = "none" :: Wrapping} :: Node,
        {kind = "text" :: "text", text = tostring(value), wrapping = "none" :: Wrapping} :: Node,
    }}
end

local function card(value: Identity, editing: boolean, t: Translate): Node
    local navi = value.navi
    if not navi then
        local stats: {Node} = {}
        if value.hp then table.insert(stats, stat(t("navi-base-hp"), value.hp)) end
        return {kind = "row", children = stats, spacing = 14, align_y = "center" :: Align}
    end
    local stats: {Node} = {}
    if value.hp then table.insert(stats, stat(t("navi-base-hp"), value.hp)) end
    if value.buster then
        table.insert(stats, stat(t("navi-buster-attack"), value.buster.attack))
        table.insert(stats, stat(t("navi-buster-rapid"), value.buster.speed))
        table.insert(stats, stat(t("navi-buster-charge"), value.buster.charge))
    end
    local name_text: Node = {kind = "text", text = navi.name or ("Navi #" .. tostring(navi.id)), size = "title" :: TextSize, wrapping = "none" :: Wrapping}
    local name: Node
    if editing then
        local children: {Node} = {name_text,
            {kind = "text", text = "\u{e1f9}", font = "icons" :: Font, size = "caption" :: TextSize, color = "muted" :: Color}}
        name = {kind = "row" :: "row", spacing = 6, align_y = "center" :: Align, children = children} :: Node
    else name = name_text end
    return {kind = "row", width = "fill" :: Size, spacing = 10, align_y = "center" :: Align, children = {
        ui.image(navi.emblem, 45, 45),
        {kind = "column" :: "column", width = "fill" :: Size, spacing = 4, children = {name :: Node,
            {kind = "wrap" :: "wrap", width = "fill" :: Size, spacing = 12, children = stats} :: Node}},
    }}
end

local function identity(value: Identity, change_action: string?, actions: Node, t: Translate): Node
    local content = card(value, change_action ~= nil, t)
    local target: Node = if change_action then
        {kind = "button" :: "button", id = change_action, width = "fill" :: Size, child = content, content_padding = {4, 6, 4, 6}, tone = "transparent" :: Tone}
        else {kind = "container" :: "container", width = "fill" :: Size, padding = {4, 6, 4, 6}, child = content}
    return {kind = "container", padding = 6, width = "fill" :: Size, tone = "card" :: Tone,
        child = {kind = "row" :: "row", width = "fill" :: Size, align_y = "center" :: Align, children = {
            {kind = "container" :: "container", width = "fill" :: Size, clip = true, child = target} :: Node,
            {kind = "container" :: "container", padding = {0, 4, 0, 4}, child = actions} :: Node,
        }}}
end

local function cell(navi: Entry, selected: boolean): Node
    local accent = navi.accent or {107 / 255, 122 / 255, 153 / 255, 1}
    local border: Color = {accent[1], accent[2], accent[3], if selected then 0.9 else 0.3}
    local shadow: Shadow? = if selected then {color = {accent[1], accent[2], accent[3], 0.5}, blur = 16} else nil
    local image = ui.image(navi.emblem, 48, 48)
    if image.kind == "image" then image.opacity = if selected then 1 else 0.45 end
    local plate: Node = {kind = "container", width = 72, height = 72, align_x = "center" :: Align, align_y = "center" :: Align,
        child = image, appearance = {background = {theme = "background", tint = accent, mix = if selected then 0.4 else 0.14} :: Color,
            radius = 36, border_width = if selected then 2 else 1, border_color = border, shadow = shadow}}
    local content: Node = {kind = "column", width = CELL_WIDTH, spacing = 6, align_x = "center" :: Align, children = {
        plate, {kind = "text" :: "text", text = navi.name or ("Navi #" .. tostring(navi.id)), size = "caption" :: TextSize,
            color = if selected then nil else "muted" :: Color},
    }}
    return {kind = "button", id = "navi.select:" .. tostring(navi.id), content_padding = CELL_PADDING, child = content,
        tone = "transparent" :: Tone, appearance = {text_color = "text" :: Color},
        hovered = {background = "transparent" :: Color}, pressed = {background = "transparent" :: Color}}
end

local function picker(roster: {{Entry}}, current: number?, t: Translate): Node
    local rows: {Node} = {}
    for _, entries in roster do
        local cells: {Node} = {}
        for _, entry in entries do table.insert(cells, cell(entry, entry.id == current)) end
        -- Keep authored roster groups and center short rows, but allow every
        -- card to remain reachable in narrow embedded panels.
        table.insert(rows, {kind = "wrap", width = "fill" :: Size, spacing = CELL_GAP,
            max_width = math.max(1, #cells * (CELL_WIDTH + 2 * CELL_PADDING) + (#cells - 1) * CELL_GAP), children = cells} :: Node)
    end
    local content: Node = {kind = "column", width = "fill" :: Size, spacing = 16, align_x = "center" :: Align, children = {
        {kind = "text" :: "text", text = t("navi-edit-select"), color = "muted" :: Color} :: Node,
        {kind = "column" :: "column", width = "fill" :: Size, spacing = 14, align_x = "center" :: Align, children = rows} :: Node,
    }}
    return {kind = "container", width = "fill" :: Size, height = "fill" :: Size, tone = "card" :: Tone,
        child = {kind = "scroll" :: "scroll", width = "fill" :: Size, height = "fill" :: Size,
            child = {kind = "container" :: "container", padding = 12, width = "fill" :: Size, child = content}}}
end

return {identity = identity, picker = picker}
