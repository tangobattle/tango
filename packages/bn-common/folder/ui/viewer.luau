--!strict
local ui = require("@editor-common/ui")
local popover = require("@editor-common/popover")
local display = require("../display")
local catalog = require("../catalog")
local rules = require("../rules")
local rows = require("./rows")

local function badge(label: string, color: Color): Node
    return {kind = "container", padding = {1, 4, 1, 4}, appearance = {background = color, radius = 3},
        child = {kind = "text" :: "text", text = label, size = 10, color = {1, 1, 1, 1} :: Color}}
end

local function row(item: display.Item, assets: catalog.Assets, grouped: boolean, index: number, issues: {rules.Issue}, t: Translate, show_code: boolean?): Node
    local chip = item.chip
    local info = if chip then assets.chip(chip.id) else nil
    local accent: Color? = rows.accent(info)
    local indicators: {Node} = {}
    if item.regular then table.insert(indicators, badge("REG", rows.REG)) end
    if item.tag1 then table.insert(indicators, badge("TAG1", rows.TAG)) end
    if item.tag2 then table.insert(indicators, badge("TAG2", rows.TAG)) end
    local title: Node = {kind = "text", text = if not chip then "—" else if info and info.name then info.name else "???",
        color = if not chip then "muted" :: Color else nil}
    local children: {Node} = {}
    if grouped then
        table.insert(children, {kind = "text", text = tostring(item.count) .. "×", width = 22,
            color = (if #issues > 0 then "danger" else if item.count == 1 then "muted" else "text") :: Color} :: Node)
    end
    table.insert(children, rows.icon(if info then info.icon() else nil))
    table.insert(children, {kind = "container", width = "fill" :: Size, child = {
        kind = "row" :: "row", spacing = 8, align_y = "center" :: Align, children = {title,
            {kind = "row" :: "row", spacing = 4, align_y = "center" :: Align, children = indicators} :: Node}}} :: Node)
    local element = if info then assets.element_icon(info.element) else nil
    table.insert(children, if element then rows.icon(element) else {kind = "space", width = 28} :: Node)
    if chip and show_code ~= false then table.insert(children, ui.right(rows.code(chip.code), 22, "body", "mono")) end
    table.insert(children, ui.right(if info and info.attack > 0 then tostring(info.attack) else "", 50, "body", "normal"))
    table.insert(children, if assets.chips_have_mb then
        ui.right(if info and info.mb > 0 then tostring(info.mb) .. "MB" else "", 50, "caption", "normal")
        else {kind = "space", width = 0} :: Node)
    local content: Node = {kind = "row", width = "fill" :: Size, spacing = 10, padding = {3, 12, 3, 12}, align_y = "center" :: Align, children = children}
    local reasons: {string} = {}
    for _, issue in issues do table.insert(reasons, rules.reason(issue, t)) end
    local tip = if chip then popover.detail(if info and info.class ~= "program_advance" then info.image() else nil,
        if info then info.description else nil, accent, reasons) else nil
    return {kind = "container", width = "fill" :: Size, tone = {row = index} :: Tone, tooltip = tip,
        appearance = {text_color = if #issues > 0 then "danger" :: Color else nil},
        child = {kind = "row" :: "row", width = "fill" :: Size, children = {ui.stripe(accent),
            {kind = "container" :: "container", width = "fill" :: Size, child = content} :: Node}}}
end

local function view(folder: display.View, assets: catalog.Assets, limits: rules.Limits, grouped: boolean, t: Translate): Node
    local rule_assets = catalog.rule_assets(assets)
    local usage = rules.scan(folder, rule_assets)
    local children: {Node} = {}
    for _, item in display.items(folder, grouped) do
        if not grouped and not item.chip then continue end
        table.insert(children, row(item, assets, grouped, #children + 1, display.issues(item, folder, rule_assets, usage, limits), t))
    end
    return {kind = "container", width = "fill" :: Size, tone = "card" :: Tone,
        child = {kind = "column" :: "column", width = "fill" :: Size, spacing = 1, children = children}}
end

return {view = view, row = row, group_control = function(grouped: boolean, t: Translate): Node
    return {kind = "checkbox", id = "folder.grouped", text = t("folder-group"), checked = grouped, size = 13, text_size = 12}
end}
