--!strict
local model = require("./model")
local catalog = require("./catalog")
export type View = model.View & {
    read set_use_count: (number, number, boolean) -> boolean,
    read rebuild: () -> (),
}

local function read(state: ViewState): {sort: catalog.Sort, filter: string}
    local sort: catalog.Sort = "id"
    for _, choice in catalog.sorts do if choice == state["auto_battle_data.sort"] then sort = choice end end
    return {sort = sort, filter = state["auto_battle_data.filter"] or ""}
end

local function update(view: View, count: number, state: ViewState, action: Action): boolean
    if action.kind == "change" then
        if action.id == "auto_battle_data.filter" then state[action.id] = action.value; return true end
        if action.id == "auto_battle_data.sort" then
            if not table.find(catalog.sorts, action.value :: catalog.Sort) then return false end
            state[action.id] = action.value
            return true
        end
        local kind, encoded_id = string.match(action.id, "^auto_battle_data%.(%a+):(%d+)$")
        local id = if encoded_id then tonumber(encoded_id) else nil
        if id == nil or id >= count or (kind ~= "used" and kind ~= "secondary") then return false end
        -- Match native count editing: first five ASCII digits, clamped to u16.
        local digits = string.sub(string.gsub(action.value, "[^0-9]", ""), 1, 5)
        if not view.set_use_count(id, math.min(tonumber(digits) or 0, 65535), kind == "secondary") then return false end
        view.rebuild()
        return true
    elseif action.kind == "activate" and action.id == "auto_battle_data.clear" then
        for id = 0, count - 1 do
            assert(view.set_use_count(id, 0, false) and view.set_use_count(id, 0, true))
        end
        view.rebuild()
        return true
    end
    return false
end

return {read = read, update = update}
