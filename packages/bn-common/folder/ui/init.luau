--!strict
local ui = require("@editor-common/ui")
local popover = require("@editor-common/popover")
local rows = require("./rows")
local catalog = require("../catalog")
local rules = require("../rules")
local edit = require("../edit")
local controller = require("../state")

export type Options = {regular: boolean, tags: boolean}

local function tooltip(info: catalog.Chip?, issues: {rules.Issue}, t: Translate, unavailable: string?): Tooltip?
    local reasons: {string} = {}
    for _, issue in issues do table.insert(reasons, rules.reason(issue, t)) end
    if #reasons == 0 and unavailable then table.insert(reasons, unavailable) end
    return popover.detail(if info and info.class ~= "program_advance" then info.image() else nil,
        if info then info.description else nil, rows.accent(info), reasons)
end

local function view(folder: edit.View, assets: catalog.Assets, limits: rules.Limits, state: ViewState,
    options: Options, t: Translate): Node
    local rule_assets = catalog.rule_assets(assets)
    local current = controller.read(state, folder)
    local regular = folder.regular()
    local usage = rules.scan(folder, rule_assets)
    local filled = 0
    local slots: {Node} = {}
    for slot = 1, folder.size do
        local chip = folder.chip(slot)
        local info = if chip then assets.chip(chip.id) else nil
        local content = rows.inner(assets, info, if chip then chip.code else nil)
        assert(content.kind == "row")
        if chip then
            filled += 1
            if options.regular then
                table.insert(content.children, ui.toggle("folder.regular:" .. tostring(slot), "REG", regular == slot,
                    rows.REG, edit.regular_allowed(folder, rule_assets, limits, regular, current.tags, slot)))
            end
            if options.tags then
                table.insert(content.children, ui.toggle("folder.tag:" .. tostring(slot), "TAG", table.find(current.tags, slot) ~= nil,
                    rows.TAG, edit.tag_allowed(folder, rule_assets, limits, regular, current.tags, slot)))
            end
            table.insert(content.children, ui.remove_button("folder.remove:" .. tostring(slot)))
        end
        content.padding = {3, 12, 3, 12}
        local issues = rules.slot_issues(folder, rule_assets, usage, slot, limits,
            {regular = regular, tags = if #current.tags == 2 then current.tags else nil})
        if chip then content.tooltip = tooltip(info, issues, t, nil) end
        local grip: Node = if chip then {kind = "container" :: "container", width = 16, align_x = "center" :: Align, cursor = "grab" :: Cursor,
            child = {kind = "text" :: "text", text = "\u{e0eb}", font = "icons" :: Font, color = "muted" :: Color}}
            else {kind = "space" :: "space", width = 16}
        local row = ui.row({{kind = "container" :: "container", padding = {0, 6, 0, 6}, child = grip}, ui.stripe(rows.accent(info)), content}, 0)
        assert(row.kind == "row")
        row.tone = {row = slot} :: Tone
        if #issues ~= 0 then row.appearance = {text_color = "danger" :: Color} :: Appearance end
        table.insert(slots, row)
    end
    local header = ui.row({{kind = "text" :: "text", text = t("folder-edit-folder")},
        ui.caption(t("folder-edit-count", {count = filled, limit = folder.size}), filled < folder.size),
        {kind = "space" :: "space", width = "fill" :: Size}, ui.clear_button("folder.clear", t("save-edit-clear"))}, 8)
    local stats: {Node} = {}
    for _, class in {"navi", "mega", "giga", "dark"} do
        local limit = if class == "navi" then limits.navi_limit elseif class == "mega" then limits.mega_limit
            elseif class == "giga" then limits.giga_limit else limits.dark_limit
        local used = if class == "navi" then usage.navi elseif class == "mega" then usage.mega
            elseif class == "giga" then usage.giga else usage.dark
        if limit then table.insert(stats, ui.caption(t("folder-edit-" .. class, {used = used, limit = limit}), used > limit)) end
    end
    if limits.reg_memory then table.insert(stats, ui.caption(t("folder-edit-reg-memory", {mb = limits.reg_memory}), false)) end
    if limits.tag_memory then table.insert(stats, ui.caption(t("folder-edit-tag-memory", {mb = limits.tag_memory}), false)) end
    local folder_header: Node = {kind = "container" :: "container", width = "fill" :: Size, padding = {8, 12, 8, 12},
        child = ui.column({header, ui.row(stats, 12)}, 4)}
    local folder_list: Node = {kind = "list" :: "list", id = "folder.slots", reorder = true, width = "fill" :: Size, spacing = 1, children = slots}

    local choices: {Choice} = {}
    for _, sort in catalog.sorts do table.insert(choices, {value = sort, label = t("folder-sort-" .. sort)}) end
    local library: {Node} = {}
    for index, entry in catalog.entries(assets, current.sort, current.filter) do
        local info = assets.chip(entry.id)
        local issues = if filled >= folder.size then {} else rules.chip_issues(usage, rule_assets, entry.id, limits, 1)
        local enabled = filled < folder.size and #issues == 0
        local content = rows.inner(assets, info, entry.code)
        assert(content.kind == "row")
        content.padding = {3, 12, 3, 12}
        local value = tostring(entry.id) .. ":" .. tostring(entry.code)
        local item: Node = {kind = "button" :: "button", id = "folder.add:" .. value, value = value,
            enabled = enabled, width = "fill" :: Size, tone = {row = index} :: Tone, content_padding = 0,
            child = ui.row({ui.stripe(rows.accent(info)), content}, 0)}
        local tip = tooltip(info, issues, t, if not enabled then t("folder-cannot-add-full") else nil)
        if not enabled then
            item = ui.unavailable(item, tip)
        else
            assert(item.kind == "button")
            item.tooltip = tip
        end
        table.insert(library, item)
    end
    local library_header = ui.library_header(t("folder-edit-search"), current.filter, choices, current.sort,
        t("save-edit-sort"), "folder.filter", "folder.sort")
    local panes = ui.row({ui.pane(folder_header, folder_list), ui.pane(library_header, ui.column(library, 1))}, 8)
    assert(panes.kind == "row")
    panes.height = "fill" :: Size
    return panes
end

return {view = view}
