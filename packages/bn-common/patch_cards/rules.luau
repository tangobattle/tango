--!strict
local catalog = require("./catalog")
local edit = require("./edit")
local MAX_MEMORY = 80
export type Usage = {memory: number, registered: {[number]: boolean}, overflow: {[number]: boolean}}

local function scan(view: edit.View, assets: catalog.Assets): Usage
    local result: Usage = {memory = 0, registered = {}, overflow = {}}
    for slot = 1, view.count() do
        local card = view.card(slot)
        if not card then continue end
        result.registered[card.id] = true
        if not card.enabled then continue end
        local info = assets.card(card.id)
        local mb = if info then info.mb else 0
        result.memory += mb
        -- Only the contributing suffix is implicated, preserving card order.
        if mb > 0 and result.memory > MAX_MEMORY then result.overflow[slot] = true end
    end
    return result
end

local function reason(mb: number, used: number, t: Translate): string
    return t("build-violation-patch-card-exceeds-memory-with-contribution", {mb = mb, used = used, limit = MAX_MEMORY})
end

return {scan = scan, reason = reason, MAX_MEMORY = MAX_MEMORY}
