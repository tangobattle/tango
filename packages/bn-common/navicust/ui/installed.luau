--!strict
local types = require("../types")
local colors = require("./colors")
local popover = require("@editor-common/popover")

local function view(parts: types.View, assets: types.Assets, illegal: {[number]: boolean}, t: Translate): Node?
    local solid, plus = {} :: {Node}, {} :: {Node}
    for slot = 1, parts.count() do
        local part = parts.part(slot)
        if not part then continue end
        local info = assets.part(part.id)
        if not info then continue end
        local bg: Color = if info.color then colors.color(info.color, not info.solid)
            else if info.solid then {189 / 255, 189 / 255, 189 / 255, 1} else {136 / 255, 136 / 255, 136 / 255, 1}
        local issues: {string} = if illegal[slot] then {t("build-violation-navicust-invalid-shape-reason")} else {}
        local badge: Node = {kind = "container", width = "fill" :: Size, padding = {3, 8, 3, 8},
            appearance = {background = bg, radius = 6, border_color = if illegal[slot] then "danger" :: Color else nil,
                border_width = if illegal[slot] then 2 else 0}, tooltip = popover.detail(nil, info.description, nil, issues),
            child = {kind = "text" :: "text", text = info.name or ("#" .. tostring(part.id)), color = {0, 0, 0, 1}}}
        table.insert(if info.solid then solid else plus, badge)
    end
    if #solid + #plus == 0 then return nil end
    return {kind = "row", spacing = 12, children = {
        {kind = "column" :: "column", width = 180, spacing = 4, children = solid},
        {kind = "column" :: "column", width = 180, spacing = 4, children = plus},
    }}
end

return {view = view}
