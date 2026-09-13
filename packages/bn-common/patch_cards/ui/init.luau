--!strict
local ui = require("@editor-common/ui")
local popover = require("@editor-common/popover")
local catalog = require("../catalog")
local edit = require("../edit")
local rules = require("../rules")
local controller = require("../state")
local rows = require("./rows")

local function tip(info: catalog.Info?, memory: number, danger: boolean, t: Translate): Tooltip?
    return if danger then popover.detail(nil, nil, nil, {rules.reason(if info then info.mb else 0, memory, t)}) else nil
end

local function view(cards: edit.View, assets: catalog.Assets, state: ViewState, t: Translate): Node
    local count = cards.count()
    local usage = rules.scan(cards, assets)
    local current = controller.read(state)
    local registered: {Node} = {}
    for slot = 1, count do
        local card = cards.card(slot)
        if not card then continue end
        local info = assets.card(card.id)
        local danger = usage.overflow[slot] == true
        local children: {Node} = {
            {kind = "container", width = 16, align_x = "center" :: Align, cursor = "grab" :: Cursor,
                child = {kind = "text" :: "text", text = "\u{e0eb}", font = "icons" :: Font, color = "muted" :: Color}},
            {kind = "text", text = string.format("%2d", slot), size = "caption" :: TextSize, width = 24},
        }
        for _, cell in rows.cells(info, card.id, card.enabled, danger) do table.insert(children, cell) end
        table.insert(children, ui.remove_button("patch_cards.remove:" .. tostring(slot)))
        table.insert(registered, {kind = "row", width = "fill" :: Size, children = children, spacing = 8,
            align_y = "start" :: Align, padding = {6, 10, 6, 6}, tone = {row = slot} :: Tone,
            appearance = if danger then {text_color = "danger" :: Color} else nil,
            tooltip = tip(info, usage.memory, danger, t)} :: Node)
    end
    local header: Node = {kind = "container" :: "container", width = "fill" :: Size, padding = {8, 12, 8, 12}, child = ui.row({
        {kind = "text" :: "text", text = t("save-tab-patch-cards")},
        ui.caption(t("patch-card-edit-count", {count = count}), false),
        ui.caption(t("patch-card-edit-mb", {mb = usage.memory, limit = rules.MAX_MEMORY}), usage.memory > rules.MAX_MEMORY),
        {kind = "space" :: "space", width = "fill" :: Size}, ui.clear_button("patch_cards.clear", t("save-edit-clear")),
    }, 8)}
    local list: Node = {kind = "list", id = "patch_cards.slots", reorder = true, width = "fill" :: Size, spacing = 3, children = registered}

    local library: {Node} = {}
    for index, entry in catalog.entries(assets, current.sort, current.filter, usage.registered) do
        local info = assets.card(entry.id)
        local enabled = count < assets.num_cards
        local memory = usage.memory + entry.mb
        local danger = enabled and memory > rules.MAX_MEMORY
        local content = ui.row(rows.cells(info, entry.id, enabled, danger), 8)
        assert(content.kind == "row")
        content.align_y = "start" :: Align
        table.insert(library, {kind = "button", id = "patch_cards.add:" .. tostring(entry.id), enabled = enabled,
            width = "fill" :: Size, tone = {row = index} :: Tone, content_padding = {6, 10, 6, 10}, child = content,
            appearance = if danger then {text_color = "danger" :: Color} else nil,
            tooltip = tip(info, memory, danger, t)} :: Node)
    end
    local choices: {Choice} = {}
    for _, sort in catalog.sorts do table.insert(choices, {value = sort, label = t("patch-card-sort-" .. sort)}) end
    local library_header = ui.library_header(t("patch-card-edit-search"), current.filter, choices, current.sort,
        t("save-edit-sort"), "patch_cards.filter", "patch_cards.sort")
    local panes = ui.row({ui.pane(header, list), ui.pane(library_header, ui.column(library, 3))}, 8)
    assert(panes.kind == "row")
    panes.height = "fill" :: Size
    return panes
end

return {view = view}
