--!strict
local ui = require("@editor-common/ui")
local catalog = require("../catalog")

local function badge(effect: catalog.Effect, enabled: boolean): Node
    local background: Color = if not enabled then {189 / 255, 189 / 255, 189 / 255, 1}
        elseif effect.debuff then {181 / 255, 90 / 255, 222 / 255, 1} else {1, 189 / 255, 24 / 255, 1}
    return {kind = "container" :: "container", width = "fill" :: Size, padding = {3, 8, 3, 8},
        appearance = {background = background, radius = 6},
        child = {kind = "text" :: "text", text = effect.name or "???", color = {0, 0, 0, 1} :: Color}}
end

local function cells(info: catalog.Info?, id: number, enabled: boolean, danger: boolean): {Node}
    local color: Color? = if not enabled then "muted" elseif danger then "danger_strong" else nil
    local name: Node = {kind = "text", text = if info and info.name then info.name else "#" .. tostring(id),
        strikethrough = not enabled, color = color}
    local mb: Node = {kind = "text", text = tostring(if info then info.mb else 0) .. "MB",
        size = 10, color = (if danger then "danger_strong" else "muted") :: Color}
    local parameters: {Node}, abilities: {Node} = {}, {}
    if info then
        for _, effect in info.effects do table.insert(if effect.ability then abilities else parameters, badge(effect, enabled)) end
    end
    return {{kind = "container" :: "container", width = "fill" :: Size, child = ui.column({name, mb}, 2)},
        {kind = "container" :: "container", width = 180, child = ui.column(parameters, 2)},
        {kind = "container" :: "container", width = 180, child = ui.column(abilities, 2)}}
end

return {cells = cells}
