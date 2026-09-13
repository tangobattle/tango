--!strict
local types = require("./types")
local MAX_COPIES = 9
export type Edit = {kind: "add", part: types.Placement} | {kind: "remove", slot: number} | {kind: "clear"}

local function counts(view: types.View): {[number]: number}
    local result: {[number]: number} = {}
    for slot = 1, view.count() do
        local part = view.part(slot)
        if part then result[part.id] = (result[part.id] or 0) + 1 end
    end
    return result
end

local function apply(view: types.View, action: Edit): boolean
    local count = view.count()
    if action.kind == "add" then
        if (counts(view)[action.part.id] or 0) >= MAX_COPIES then return false end
        local free: number? = nil
        for slot = 1, count do if not view.part(slot) then free = slot; break end end
        if free == nil or not view.set_part(free, action.part) then return false end
    elseif action.kind == "remove" then
        local slot = action.slot
        if slot < 1 or slot > count or slot ~= math.floor(slot) then return false end
        for index = 1, count - 1 do assert(view.set_part(index, view.part(if index < slot then index else index + 1))) end
        assert(view.set_part(count, nil))
    else
        for slot = 1, count do assert(view.set_part(slot, nil)) end
    end
    view.rebuild()
    return true
end

return {apply = apply, counts = counts, MAX_COPIES = MAX_COPIES}
