--!strict
-- A package supplies the change token; the renderer owns elapsed time.
local function enter(node: Node, key: string, revision: string, from: {number}): Node
    local result = table.clone(node)
    result.motion = {key = key, revision = revision, from = from,
        duration_ms = 160, easing = "ease_out_cubic" :: Easing} :: Motion
    return result
end

return {enter = enter}
