--!strict
local catalog = require("./catalog")
local edit = require("./edit")

local function text(cards: edit.View, assets: catalog.Assets): string
    local output: {string} = {}
    for slot = 1, cards.count() do
        local card = cards.card(slot)
        if not card or not card.enabled then continue end
        local info = assets.card(card.id)
        local name = if info and info.name then info.name else "#" .. tostring(card.id)
        table.insert(output, name .. "\t" .. tostring(if info then info.mb else 0) .. "MB\n")
    end
    return table.concat(output)
end

return {text = text}
