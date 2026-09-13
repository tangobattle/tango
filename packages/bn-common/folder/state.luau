--!strict
local edit = require("./edit")
local rules = require("./rules")
local catalog = require("./catalog")

export type State = {tags: {number}, filter: string, sort: catalog.Sort}

local function read(state: ViewState, view: edit.View): State
    local tags: {number} = {}
    local stored = state["folder.tags"]
    if stored == nil then
        tags = table.clone(view.tags() or {})
    else
        for value in string.gmatch(stored, "%d+") do
            local slot = tonumber(value) :: number
            if slot >= 1 and slot <= view.size and not table.find(tags, slot) and #tags < 2 then table.insert(tags, slot) end
        end
    end
    local sort: catalog.Sort = "id"
    for _, option in catalog.sorts do if state["folder.sort"] == option then sort = option :: catalog.Sort end end
    return {tags = tags, filter = state["folder.filter"] or "", sort = sort}
end

local function write_tags(state: ViewState, tags: {number})
    local values: {string} = {}
    for _, slot in tags do table.insert(values, tostring(slot)) end
    state["folder.tags"] = table.concat(values, ",")
end

-- The UI's transient selection follows the same movement as the stored pair.
-- A single pending Tag remains selected even though the save stores no pair.
local function update(state: ViewState, view: edit.View, assets: rules.Assets, limits: rules.Limits, action: Action): boolean
    if action.kind == "change" then
        if action.id == "folder.filter" then state[action.id] = action.value; return true end
        if action.id == "folder.sort" then
            assert(table.find(catalog.sorts, action.value :: catalog.Sort), "Unknown folder sort")
            state[action.id] = action.value
            return true
        end
        return false
    end
    local current = read(state, view)
    local tags = current.tags
    local operation: edit.Action? = nil
    if action.kind == "reorder" and action.id == "folder.slots" then
        local to = edit.drop_target(view, action.from, action.to)
        if to == nil then return true end
        tags = edit.remap_staged(tags, function(slot: number): number return edit.reorder_index(slot, action.from, to) end)
        operation = {kind = "move", from = action.from, to = to}
    elseif action.kind == "activate" then
        if action.id == "folder.clear" then
            tags = {}
            operation = {kind = "clear"} :: edit.Action
        elseif action.id == "folder.add:" .. action.value then
            local id_text, code_text = string.match(action.value, "^(%d+):(%d+)$")
            if not id_text or not code_text then return false end
            local id, code = tonumber(id_text) :: number, tonumber(code_text) :: number
            local info = assets.chip(id)
            if not info or not table.find(info.codes, code) or not assets.chip_is_legal(id) then return true end
            local gap: number? = nil
            for slot = 1, view.size do if not view.chip(slot) then gap = slot; break end end
            if gap == nil or #rules.chip_issues(rules.scan(view, assets), assets, id, limits, 1) ~= 0 then return true end
            tags = edit.remap_staged(tags, function(slot: number): number return if slot < (gap :: number) then slot + 1 else slot end)
            operation = {kind = "add", chip = {id = id, code = code}}
        else
            local name, slot_text = string.match(action.id, "^folder%.(%a+):(%d+)$")
            if not name or not slot_text then return false end
            local slot = tonumber(slot_text) :: number
            if slot < 1 or slot > view.size or not view.chip(slot) then return true end
            if name == "remove" then
                tags = edit.remap_staged(tags, function(index: number): number?
                    return if index == slot then nil elseif index > slot then index - 1 else index
                end)
                operation = {kind = "remove", slot = slot} :: edit.Action
            elseif name == "regular" then
                if not edit.regular_allowed(view, assets, limits, view.regular(), tags, slot) then return true end
                operation = {kind = "regular", slot = slot} :: edit.Action
            elseif name == "tag" then
                if not edit.tag_allowed(view, assets, limits, view.regular(), tags, slot) then return true end
                tags = edit.toggle_tag(tags, slot)
                operation = {kind = "tags", pair = if #tags == 2 then tags else nil} :: edit.Action
            end
        end
    end
    if not operation then return false end
    edit.apply(view, assets, limits, operation)
    write_tags(state, tags)
    return true
end

return {read = read, update = update}
