--!strict
local rules = require("./rules")

export type View = rules.View & {
    regular: () -> number?, tags: () -> {number}?,
    set_chip: (number, rules.Chip?) -> boolean,
    set_regular: (number?) -> boolean, set_tags: ({number}?) -> boolean,
    rebuild_anticheat: () -> (),
}
export type Action =
    {kind: "add", chip: rules.Chip}
    | {kind: "remove", slot: number}
    | {kind: "move", from: number, to: number}
    | {kind: "clear"}
    | {kind: "regular", slot: number}
    | {kind: "tags", pair: {number}?}
    | {kind: "set", slot: number, chip: rules.Chip?}

local function valid_slot(slot: number, size: number): boolean
    return slot >= 1 and slot <= size and slot == math.floor(slot)
end

local function reorder_index(index: number, from: number, to: number): number
    if index == from then return to end
    if from < to and index > from and index <= to then return index - 1 end
    if from > to and index >= to and index < from then return index + 1 end
    return index
end

local function remap_tags(tags: {number}?, remap: (number) -> number?): {number}?
    if not tags then return nil end
    local a, b = remap(tags[1]), remap(tags[2])
    return if a ~= nil and b ~= nil then {a, b} else nil
end

local function regular_allowed(view: rules.View, assets: rules.Assets, limits: rules.Limits,
    regular: number?, tags: {number}, slot: number): boolean
    if not valid_slot(slot, view.size) then return false end
    return regular == slot or (not table.find(tags, slot)
        and (limits.reg_memory == nil or rules.mb_at(view, assets, slot) <= limits.reg_memory))
end

local function tag_allowed(view: rules.View, assets: rules.Assets, limits: rules.Limits,
    regular: number?, tags: {number}, slot: number): boolean
    if not valid_slot(slot, view.size) then return false end
    if table.find(tags, slot) then return true end
    if regular == slot or #tags >= 2 then return false end
    local mb = rules.mb_at(view, assets, slot)
    local partner = if #tags == 1 then rules.mb_at(view, assets, tags[1]) else 0
    return limits.tag_memory == nil or (mb <= limits.tag_memory and mb + partner <= limits.tag_memory)
end

local function apply(view: View, assets: rules.Assets, limits: rules.Limits, action: Action)
    -- Class/copy findings are warnings, not edit guards. Save is blocked by
    -- the build report; Regular and Tag memory have their own edit gates.
    if action.kind == "regular" then
        if not valid_slot(action.slot, view.size) then return end
        if view.regular() ~= action.slot and limits.reg_memory
            and rules.mb_at(view, assets, action.slot) > limits.reg_memory then return end
    elseif action.kind == "tags" and action.pair then
        local pair = action.pair
        if #pair ~= 2 or not valid_slot(pair[1], view.size) or not valid_slot(pair[2], view.size) then return end
        if limits.tag_memory and rules.mb_at(view, assets, pair[1]) + rules.mb_at(view, assets, pair[2]) > limits.tag_memory then return end
    end
    local regular, tags = view.regular(), view.tags()
    local folder: {[number]: rules.Chip?} = {}
    for slot = 1, view.size do folder[slot] = view.chip(slot) end
    local function write_folder(next_regular: number?, next_tags: {number}?)
        for slot = 1, view.size do assert(view.set_chip(slot, folder[slot]), "Cannot write folder slot") end
        -- Preserve the native writer's setter order, including the cartridge
        -- view's conflict checks between the stored Regular and Tag slots.
        view.set_regular(next_regular)
        view.set_tags(next_tags)
    end
    if action.kind == "add" then
        local gap: number? = nil
        for slot = 1, view.size do if not folder[slot] then gap = slot; break end end
        if gap == nil then return end
        for slot = gap, 2, -1 do folder[slot] = folder[slot - 1] end
        folder[1] = action.chip
        local function remap(slot: number): number return if slot < (gap :: number) then slot + 1 else slot end
        write_folder(if regular then remap(regular) else nil, remap_tags(tags, remap))
    elseif action.kind == "remove" then
        if not valid_slot(action.slot, view.size) then return end
        for slot = action.slot, view.size - 1 do folder[slot] = folder[slot + 1] end
        folder[view.size] = nil
        local function remap(slot: number): number?
            return if slot == action.slot then nil elseif slot > action.slot then slot - 1 else slot
        end
        write_folder(if regular then remap(regular) else nil, remap_tags(tags, remap))
    elseif action.kind == "move" then
        local from, to = action.from, action.to
        if not valid_slot(from, view.size) or not valid_slot(to, view.size) or from == to
            or not folder[from] or not folder[to] then return end
        local moved = folder[from]
        if from < to then
            for slot = from, to - 1 do folder[slot] = folder[slot + 1] end
        else
            for slot = from, to + 1, -1 do folder[slot] = folder[slot - 1] end
        end
        folder[to] = moved
        local function remap(slot: number): number return reorder_index(slot, from, to) end
        write_folder(if regular then remap(regular) else nil, remap_tags(tags, remap))
    elseif action.kind == "clear" then
        table.clear(folder)
        write_folder(nil, nil)
    elseif action.kind == "regular" then
        view.set_regular(if regular == action.slot then nil else action.slot)
    elseif action.kind == "tags" then
        view.set_tags(action.pair)
    elseif action.kind == "set" then
        view.set_chip(action.slot, action.chip)
    end
    view.rebuild_anticheat()
end

-- UI scratch keeps a lone pending Tag selection, although the save can only
-- store a complete pair. Remap it alongside the save-side folder operation.
local function remap_staged(tags: {number}, remap: (number) -> number?): {number}
    local result: {number} = {}
    for _, slot in tags do
        local next_slot = remap(slot)
        if next_slot ~= nil then table.insert(result, next_slot) end
    end
    return result
end

local function toggle_tag(tags: {number}, slot: number): {number}
    local result = table.clone(tags)
    local index = table.find(result, slot)
    if index then table.remove(result, index) elseif #result < 2 then table.insert(result, slot) end
    return result
end

local function drop_target(view: rules.View, from: number, target: number): number?
    if not valid_slot(from, view.size) or not valid_slot(target, view.size) or not view.chip(from) then return nil end
    local to = target
    if not view.chip(to) then
        for slot = view.size, 1, -1 do if view.chip(slot) then to = slot; break end end
    end
    return if from == to then nil else to
end

return {apply = apply, regular_allowed = regular_allowed, tag_allowed = tag_allowed,
    reorder_index = reorder_index, remap_staged = remap_staged, toggle_tag = toggle_tag, drop_target = drop_target}
