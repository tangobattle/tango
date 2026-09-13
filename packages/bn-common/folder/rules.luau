--!strict
-- Shared Battle Network folder rules. Cartridge layouts and ROM decoding are
-- supplied by the driver. Slots are 1-based; chip IDs and codes retain their encoded values.
export type Chip = {id: number, code: number}
export type ChipInfo = {name: string?, codes: {number}, class: string, dark: boolean?, mb: number}
export type Assets = {chip: (number) -> ChipInfo?, chip_is_legal: (number) -> boolean}
export type View = {size: number, chip: (number) -> Chip?}
export type Selections = {regular: number?, tags: {number}?}
export type Limits = {
    navi_limit: number?, mega_limit: number?, giga_limit: number?, dark_limit: number?,
    reg_memory: number?, tag_memory: number?, max_copies: (ChipInfo) -> number,
}
export type Usage = {navi: number, mega: number, giga: number, dark: number, copies: {[number]: number}}
export type SimpleIssue = {kind: "chip_illegal_for_game" | "chip_code_unavailable"}
export type LimitIssue = {kind: "too_many_copies_of_chip" | "too_many_navi_chips" | "too_many_mega_chips"
        | "too_many_giga_chips" | "too_many_dark_chips" | "regular_chip_exceeds_memory"
        | "tag_chips_exceed_memory", used: number, limit: number}
export type Issue = SimpleIssue | LimitIssue
export type Finding =
    {kind: "folder_not_full", used: number, required: number}
    | {kind: "chip", slot: number, id: number, code: number, issue: Issue}

local function scan(view: View, assets: Assets): Usage
    local usage: Usage = {navi = 0, mega = 0, giga = 0, dark = 0, copies = {}}
    for slot = 1, view.size do
        local chip = view.chip(slot)
        if not chip then continue end
        usage.copies[chip.id] = (usage.copies[chip.id] or 0) + 1
        local info = assets.chip(chip.id)
        if not info then continue end
        if info.dark then usage.dark += 1
        elseif info.class == "navi" then usage.navi += 1
        elseif info.class == "mega" then usage.mega += 1
        elseif info.class == "giga" then usage.giga += 1 end
    end
    return usage
end

local function chip_issues(usage: Usage, assets: Assets, id: number, limits: Limits, additional: number): {Issue}
    local issues: {Issue} = {}
    if not assets.chip_is_legal(id) then table.insert(issues, {kind = "chip_illegal_for_game" :: "chip_illegal_for_game"} :: Issue) end
    local info = assets.chip(id)
    if not info then return issues end
    local copies = (usage.copies[id] or 0) + additional
    local cap = limits.max_copies(info)
    if copies > cap then table.insert(issues, {kind = "too_many_copies_of_chip" :: "too_many_copies_of_chip", used = copies, limit = cap} :: Issue) end
    if info.dark then
        local used = usage.dark + additional
        if limits.dark_limit and used > limits.dark_limit then
            table.insert(issues, {kind = "too_many_dark_chips" :: "too_many_dark_chips", used = used, limit = limits.dark_limit} :: Issue)
        end
    elseif info.class == "navi" then
        local used = usage.navi + additional
        if limits.navi_limit and used > limits.navi_limit then
            table.insert(issues, {kind = "too_many_navi_chips" :: "too_many_navi_chips", used = used, limit = limits.navi_limit} :: Issue)
        end
    elseif info.class == "mega" then
        local used = usage.mega + additional
        if limits.mega_limit and used > limits.mega_limit then
            table.insert(issues, {kind = "too_many_mega_chips" :: "too_many_mega_chips", used = used, limit = limits.mega_limit} :: Issue)
        end
    elseif info.class == "giga" then
        local used = usage.giga + additional
        if limits.giga_limit and used > limits.giga_limit then
            table.insert(issues, {kind = "too_many_giga_chips" :: "too_many_giga_chips", used = used, limit = limits.giga_limit} :: Issue)
        end
    end
    return issues
end

local function mb_at(view: View, assets: Assets, slot: number): number
    local chip = view.chip(slot)
    local info = if chip then assets.chip(chip.id) else nil
    return if info then info.mb else 0
end

local function slot_issues(view: View, assets: Assets, usage: Usage, slot: number, limits: Limits, selections: Selections): {Issue}
    local chip = view.chip(slot)
    if not chip then return {} end
    local issues = chip_issues(usage, assets, chip.id, limits, 0)
    local info = assets.chip(chip.id)
    if not info then return issues end
    if not table.find(info.codes, chip.code) then table.insert(issues, {kind = "chip_code_unavailable" :: "chip_code_unavailable"} :: Issue) end
    if selections.regular == slot and limits.reg_memory and info.mb > limits.reg_memory then
        table.insert(issues, {kind = "regular_chip_exceeds_memory" :: "regular_chip_exceeds_memory", used = info.mb, limit = limits.reg_memory} :: Issue)
    end
    local tags = selections.tags
    if tags and table.find(tags, slot) and limits.tag_memory then
        local used = mb_at(view, assets, tags[1]) + mb_at(view, assets, tags[2])
        if used > limits.tag_memory then
            table.insert(issues, {kind = "tag_chips_exceed_memory" :: "tag_chips_exceed_memory", used = used, limit = limits.tag_memory} :: Issue)
        end
    end
    return issues
end

local function violations(view: View, assets: Assets, limits: Limits, selections: Selections): {Finding}
    local usage = scan(view, assets)
    local used = 0
    for slot = 1, view.size do if view.chip(slot) then used += 1 end end
    local findings: {Finding} = {}
    if used < view.size then table.insert(findings, {kind = "folder_not_full" :: "folder_not_full", used = used, required = view.size} :: Finding) end
    for slot = 1, view.size do
        local chip = view.chip(slot)
        if not chip then continue end
        for _, issue in slot_issues(view, assets, usage, slot, limits, selections) do
            table.insert(findings, {kind = "chip" :: "chip", slot = slot, id = chip.id, code = chip.code, issue = issue} :: Finding)
        end
    end
    return findings
end

local function reason(issue: Issue, t: Translate): string
    local key = "build-violation-" .. string.gsub(issue.kind, "_", "-")
    if issue.kind == "chip_illegal_for_game" or issue.kind == "chip_code_unavailable" then return t(key) end
    local details = issue :: LimitIssue
    return t(key, {used = details.used, limit = details.limit})
end

local function warnings(findings: {Finding}, assets: Assets, t: Translate): {string}
    local result: {string} = {}
    local chips: {{slot: number, id: number, code: number, issue: Issue}} = {}
    for _, finding in findings do
        if finding.kind == "folder_not_full" then
            table.insert(result, t("build-violation-folder-not-full", {used = finding.used, required = finding.required}))
        else
            table.insert(chips, {slot = finding.slot, id = finding.id, code = finding.code, issue = finding.issue})
        end
    end
    -- Preserve the native stable ordering by chip ID, then slot, then the
    -- original issue order. Every equivalent contributing chip stays marked.
    type Indexed = {index: number, chip: {slot: number, id: number, code: number, issue: Issue}}
    local indexed: {Indexed} = {}
    for index, chip in chips do table.insert(indexed, {index = index, chip = chip}) end
    table.sort(indexed, function(a: Indexed, b: Indexed): boolean
        if a.chip.id ~= b.chip.id then return a.chip.id < b.chip.id end
        if a.chip.slot ~= b.chip.slot then return a.chip.slot < b.chip.slot end
        return a.index < b.index
    end)
    type Group = {issue: Issue, labels: {string}, seen: {[string]: boolean}}
    local groups: {Group} = {}
    for _, item in indexed do
        local chip = item.chip
        local group_index: number? = nil
        for index, candidate in groups do
            local a, b = candidate.issue, chip.issue
            local equal = a.kind == b.kind
            if a.kind ~= "chip_illegal_for_game" and a.kind ~= "chip_code_unavailable"
                and b.kind ~= "chip_illegal_for_game" and b.kind ~= "chip_code_unavailable" then
                local x, y = a :: LimitIssue, b :: LimitIssue
                equal = equal and x.used == y.used and x.limit == y.limit
            end
            if equal then group_index = index; break end
        end
        if not group_index then
            local group: Group = {issue = chip.issue, labels = {}, seen = {}}
            table.insert(groups, group)
            group_index = #groups
        end
        local group = groups[group_index :: number]
        local identity = tostring(chip.id) .. ":" .. tostring(chip.code)
        if group.seen[identity] then continue end
        group.seen[identity] = true
        local info = assets.chip(chip.id)
        local name = if info and info.name then info.name else t("build-chip-unknown", {id = chip.id})
        table.insert(group.labels, name .. " " .. (if chip.code == 26 then "*" else string.char(65 + chip.code)))
    end
    for _, group in groups do
        table.insert(result, t("build-violation", {subject = table.concat(group.labels, ", "), reason = reason(group.issue, t)}))
    end
    return result
end

return {scan = scan, chip_issues = chip_issues, slot_issues = slot_issues, violations = violations,
    mb_at = mb_at, reason = reason, warnings = warnings}
