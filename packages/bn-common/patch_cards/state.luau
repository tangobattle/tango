--!strict
local catalog = require("./catalog")
local edit = require("./edit")

local function read(state: ViewState): {sort: catalog.Sort, filter: string}
    local sort: catalog.Sort = "id"
    for _, value in catalog.sorts do if value == state["patch_cards.sort"] then sort = value :: catalog.Sort end end
    return {sort = sort, filter = state["patch_cards.filter"] or ""}
end

local function update(state: ViewState, view: edit.View, capacity: number, action: Action): boolean
    if action.kind == "change" then
        if action.id == "patch_cards.filter" then state[action.id] = action.value; return true end
        if action.id == "patch_cards.sort" then
            assert(table.find(catalog.sorts, action.value :: catalog.Sort), "Unknown card sort")
            state[action.id] = action.value
            return true
        end
    elseif action.kind == "reorder" and action.id == "patch_cards.slots" then
        edit.apply(view, capacity, {kind = "move", from = action.from, to = action.to})
        return true
    elseif action.kind == "activate" then
        if action.id == "patch_cards.clear" then edit.apply(view, capacity, {kind = "clear"}); return true end
        local kind, value = string.match(action.id, "^patch_cards%.(%a+):(%d+)$")
        local index = if value then tonumber(value) else nil
        if index and kind == "add" then edit.apply(view, capacity, {kind = "add", id = index}); return true end
        if index and kind == "remove" then edit.apply(view, capacity, {kind = "remove", slot = index}); return true end
    end
    return false
end

return {read = read, update = update}
