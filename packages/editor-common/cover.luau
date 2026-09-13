--!strict
-- A full-height artwork panel with a reveal action. The caller owns
-- the images, label, action and the decision to conceal its document.
local function artwork(logos: {Raster}): Node
    if #logos >= 2 then
        local height = 140
        local top_width = height * logos[1].width / logos[1].height
        local bottom_width = height * logos[2].width / logos[2].height
        local lane = math.max(top_width, bottom_width) + 64
        return {kind = "column", spacing = 20, children = {
            {kind = "container" :: "container", width = lane, align_x = "start" :: Align, child = {
                kind = "image" :: "image", image = logos[1], width = top_width, height = height, nearest = false,
            }} :: Node,
            {kind = "container" :: "container", width = lane, align_x = "end" :: Align, child = {
                kind = "image" :: "image", image = logos[2], width = bottom_width, height = height, nearest = false,
            }} :: Node,
        }}
    elseif #logos == 1 then
        return {kind = "image", image = logos[1], width = "fill" :: Size, height = 220, nearest = false}
    end
    return {kind = "space"}
end

local function view(logos: {Raster}, id: string, label: string): Node
    local frame: Node = {kind = "container", width = "fill" :: Size, height = "fill" :: Size,
        align_y = "center" :: Align, padding = 36, tone = "card" :: Tone, child = {
            kind = "column" :: "column", width = "fill" :: Size, align_x = "center" :: Align,
            children = {artwork(logos)},
        }}
    local reveal: Node = {kind = "container", width = "fill" :: Size, height = "fill" :: Size,
        align_x = "end" :: Align, align_y = "end" :: Align, padding = 24, child = {
            kind = "button" :: "button", id = id, tone = "neutral" :: Tone, content_padding = {5, 12, 5, 12},
            child = {kind = "row" :: "row", spacing = 8, align_y = "center" :: Align, children = {
                {kind = "text" :: "text", text = "\u{e0ba}", font = "icons" :: Font} :: Node,
                {kind = "text" :: "text", text = label} :: Node,
            }} :: Node,
        }}
    return {kind = "stack", width = "fill" :: Size, height = "fill" :: Size, children = {frame, reveal}}
end

return {view = view}
