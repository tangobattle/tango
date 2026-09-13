--!strict
local common = require("./motion")

local function restart(state: ViewState, x: number)
    state["editor.enter.revision"] = tostring((tonumber(state["editor.enter.revision"] or "0") or 0) + 1)
    state["editor.enter.x"] = tostring(x)
    state["editor.enter.region"] = nil
end

local function body(node: Node, state: ViewState): Node
    if state["editor.enter.region"] then return node end
    local x = tonumber(state["editor.enter.x"] or "0") or 0
    return common.enter(node, "editor.enter.body", state["editor.enter.revision"] or "0", {x, if x == 0 then 20 else 0})
end

local function picker(state: ViewState)
    restart(state, 0)
    state["editor.enter.region"] = "true"
end

local function region(node: Node, state: ViewState): Node
    -- Opening, dismissing, and selecting all bring the current region in as
    -- one unit. A rapid toggle starts another entrance; no hidden UI or
    -- reversible presentation state needs to survive in the renderer.
    return if state["editor.enter.region"] then common.enter(node,
        "editor.enter.region", state["editor.enter.revision"] or "0", {0, 20}) else node
end

local function tail(node: Node, state: ViewState, key: string, carried: boolean): Node
    local x = tonumber(state["editor.enter.x"] or "0") or 0
    -- Save arrivals and edit changes leave the strip planted; tab changes move
    -- only the contextual controls, in the body's direction of travel.
    return if x == 0 or carried then node else common.enter(node, "editor.enter.tail." .. key, state["editor.enter.revision"] or "0", {x, 0})
end

return {restart = restart, body = body, tail = tail, picker = picker, region = region}
