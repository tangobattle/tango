--!strict
local rules = require("./rules")
export type View = rules.View & {regular: () -> number?, tags: () -> {number}?}
export type Item = {chip: rules.Chip?, count: number, slots: {number}, regular: boolean, tag1: boolean, tag2: boolean}

local function items(view: View, grouped: boolean): {Item}
    local result: {Item} = {}
    local indexes: {[string]: number} = {}
    local regular, tags = view.regular(), view.tags()
    for slot = 1, view.size do
        local chip = view.chip(slot)
        local key = if chip then tostring(chip.id) .. ":" .. tostring(chip.code) else "empty"
        local index = if grouped then indexes[key] else nil
        if not index then
            table.insert(result, {chip = chip, count = 0, slots = {}, regular = false, tag1 = false, tag2 = false} :: Item)
            index = #result
            indexes[key] = index
        end
        local item = result[index :: number]
        item.count += 1
        table.insert(item.slots, slot)
        item.regular = item.regular or regular == slot
        item.tag1 = item.tag1 or (tags ~= nil and tags[1] == slot)
        item.tag2 = item.tag2 or (tags ~= nil and tags[2] == slot)
    end
    return result
end

local function issues(item: Item, view: View, assets: rules.Assets, usage: rules.Usage, limits: rules.Limits): {rules.Issue}
    local result: {rules.Issue} = {}
    local seen: {[string]: boolean} = {}
    for _, slot in item.slots do
        for _, issue in rules.slot_issues(view, assets, usage, slot, limits, {regular = view.regular(), tags = view.tags()}) do
            local key: string = issue.kind
            if issue.kind ~= "chip_illegal_for_game" and issue.kind ~= "chip_code_unavailable" then
                local value = issue :: rules.LimitIssue
                key ..= ":" .. tostring(value.used) .. ":" .. tostring(value.limit)
            end
            if seen[key] then continue end
            seen[key] = true
            table.insert(result, issue)
        end
    end
    return result
end

return {items = items, issues = issues}
