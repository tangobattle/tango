--!strict
local ui = require("@editor-common/ui")
local popover = require("@editor-common/popover")
local model = require("./model")
local catalog = require("./catalog")
local controller = require("./state")
local folder = require("../folder/catalog")
local rows = require("../folder/ui/rows")
local viewer = require("../folder/ui/viewer")

local titles = {
    secondary_standard = "auto-battle-data-secondary-standard-chips",
    standard = "auto-battle-data-standard-chips", mega = "auto-battle-data-mega-chips",
    giga = "auto-battle-data-giga-chip", combos = "auto-battle-data-combos",
    program_advance = "auto-battle-data-program-advance",
}

local function section_rows(section: model.Section, assets: folder.Assets, t: Translate): Node
    local children: {Node} = {
        {kind = "container", padding = {8, 12, 8, 12}, child = {kind = "text" :: "text", text = t(titles[section.name])}},
        {kind = "space", height = 4},
    }
    for index, run in section.runs do
        table.insert(children, viewer.row({chip = if run.id then {id = run.id, code = 0} else nil,
            count = run.count, slots = {}, regular = false, tag1 = false, tag2 = false}, assets, true, index, {}, t, false))
    end
    return ui.column(children, 1)
end

local function sections(view: model.View, assets: folder.Assets): {model.Section}
    return model.group(view, catalog.ranking_assets(assets))
end

local function read_only(view: model.View, assets: folder.Assets, t: Translate): Node
    local children: {Node} = {}
    for _, section in sections(view, assets) do
        table.insert(children, {kind = "container", width = "fill" :: Size, tone = "card" :: Tone,
            child = section_rows(section, assets, t)} :: Node)
    end
    return ui.column(children, 8)
end

local function count_cell(label: string, value: number, id: string): Node
    return {kind = "container", width = 104, align_x = "end" :: Align,
        child = {kind = "row" :: "row", spacing = 4, align_y = "center" :: Align, children = {
            ui.caption(label, false),
            {kind = "input" :: "input", id = id, value = tostring(value), placeholder = "0", width = 54,
                content_padding = {4, 8, 4, 8}} :: Node,
        }}}
end

local function library_row(view: model.View, assets: folder.Assets, entry: catalog.Entry, index: number, t: Translate): Node
    local info = assets.chip(entry.id)
    assert(info)
    local accent = rows.accent(info)
    local element = assets.element_icon(info.element)
    local content: Node = {kind = "row", width = "fill" :: Size, spacing = 8, padding = {3, 12, 3, 12},
        align_y = "center" :: Align, children = {
            rows.icon(info.icon()),
            {kind = "text", text = entry.name, width = "fill" :: Size},
            if element then rows.icon(element) else {kind = "space", width = 28} :: Node,
            ui.right(if info.attack > 0 then tostring(info.attack) else "", 46, "body", "normal"),
            if assets.chips_have_mb then ui.right(if info.mb > 0 then tostring(info.mb) .. "MB" else "", 42, "caption", "normal")
                else {kind = "space", width = 0} :: Node,
            count_cell(t("auto-battle-data-edit-used"), entry.used, "auto_battle_data.used:" .. tostring(entry.id)),
            if entry.secondary then count_cell(t("auto-battle-data-edit-secondary"), view.use_count(entry.id, true),
                "auto_battle_data.secondary:" .. tostring(entry.id)) else {kind = "space", width = 104} :: Node,
        }}
    return {kind = "container", width = "fill" :: Size, tone = {row = index} :: Tone,
        tooltip = popover.detail(if info.class ~= "program_advance" then info.image() else nil, info.description, accent, {}),
        child = ui.row({ui.stripe(accent), content}, 0)}
end

local function edit(view: model.View, assets: folder.Assets, state: ViewState, t: Translate): Node
    local deck: {Node} = {}
    local seen: {[number]: boolean} = {}
    local distinct = 0
    for _, section in sections(view, assets) do
        table.insert(deck, section_rows(section, assets, t))
        for _, run in section.runs do
            if run.id and not seen[run.id] then seen[run.id] = true; distinct += 1 end
        end
    end
    local header: Node = {kind = "container", width = "fill" :: Size, padding = {8, 12, 8, 12}, child = ui.row({
        {kind = "text", text = t("save-tab-auto-battle-data")},
        ui.caption(t("auto-battle-data-edit-count", {count = distinct}), false),
        {kind = "space", width = "fill" :: Size}, ui.clear_button("auto_battle_data.clear", t("save-edit-clear")),
    }, 8)}
    local current = controller.read(state)
    local choices: {Choice} = {}
    for _, sort in catalog.sorts do
        table.insert(choices, {value = sort, label = t(if sort == "used" then "auto-battle-data-edit-used" else "folder-sort-" .. sort)})
    end
    local library: {Node} = {}
    for index, entry in catalog.entries(view, assets, current.sort, current.filter) do
        table.insert(library, library_row(view, assets, entry, index, t))
    end
    local library_header = ui.library_header(t("folder-edit-search"), current.filter, choices, current.sort,
        t("save-edit-sort"), "auto_battle_data.filter", "auto_battle_data.sort")
    local panes = ui.row({ui.pane(header, ui.column(deck, 1)), ui.pane(library_header, ui.column(library, 1))}, 8)
    assert(panes.kind == "row")
    panes.height = "fill" :: Size
    return panes
end

local function text(view: model.View, assets: folder.Assets, t: Translate): string
    local lines: {string} = {}
    for _, section in sections(view, assets) do
        table.insert(lines, "[" .. t(titles[section.name]) .. "]")
        for _, run in section.runs do
            local chip = if run.id then assets.chip(run.id) else nil
            local name = if not run.id then "---" else if chip and chip.name then chip.name else "#" .. tostring(run.id)
            table.insert(lines, tostring(run.count) .. "\t" .. name)
        end
        table.insert(lines, "")
    end
    return table.concat(lines, "\n") .. "\n"
end

return {view = edit, viewer = read_only, text = text}
