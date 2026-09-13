--!strict
local ui = require("@editor-common/ui")
local popover = require("@editor-common/popover")
local catalog = require("../catalog")
local edit = require("../edit")
local rules = require("../rules")
local rows = require("./rows")

local function view(cards: edit.View, assets: catalog.Assets, t: Translate): Node
    local usage = rules.scan(cards, assets)
    local children: {Node} = {}
    for slot = 1, cards.count() do
        local card = cards.card(slot)
        if not card then continue end
        local info = assets.card(card.id)
        local danger = usage.overflow[slot] == true
        local cells: {Node} = {{kind = "text", text = string.format("%2d", slot), size = "caption" :: TextSize, width = 24}}
        for _, cell in rows.cells(info, card.id, card.enabled, danger) do table.insert(cells, cell) end
        table.insert(children, {kind = "row", width = "fill" :: Size, children = cells, spacing = 8,
            align_y = "start" :: Align, padding = {6, 10, 6, 10}, tone = {row = slot} :: Tone,
            appearance = if danger then {text_color = "danger" :: Color} else nil,
            tooltip = if danger then popover.detail(nil, nil, nil,
                {rules.reason(if info then info.mb else 0, usage.memory, t)}) else nil} :: Node)
    end
    return {kind = "container", width = "fill" :: Size, tone = "card" :: Tone, child = ui.column(children, 3)}
end

return {view = view}
