--!strict
local ui = require("@editor-common/ui")
local popover = require("@editor-common/popover")
local cards = require("../save/patch_cards")
local catalog = require("./catalog")
local rules = require("./rules")
local labels = require("./labels")
local controller = require("./state")
local t = require("../i18n")

local function view(value: cards.Model, assets: controller.Assets, read_only: boolean?): Node
    local issues: {[number]: string} = {}
    for _, finding in rules.violations(value, assets) do
        issues[finding.slot] = labels.reason(finding)
    end
    local choices: {{Choice}} = {}
    for slot = 1, value.patch_count() do choices[slot] = {{value = "none", label = t("patch-card4-none")}} end
    if not read_only then
        for id = 1, catalog.count - 1 do
            local info = assets.patch_card(id)
            local bucket = if info then choices[info.slot] else nil
            if bucket and info then table.insert(bucket, {value = tostring(id),
                label = string.format("%03d %s — %s", id, labels.name(info, id), labels.effect(info.effect))}) end
        end
    end
    local rows: {Node} = {}
    local count = 0
    for slot = 1, value.patch_count() do
        local card = value.patch_card(slot)
        if card then count += 1 end
        local info = if card then assets.patch_card(card.id) else nil
        local badge: Node = {kind = "container" :: "container", width = 34, align_x = "center" :: Align,
            child = {kind = "text" :: "text", text = labels.slot(slot), font = "mono" :: Font, size = "body" :: TextSize}}
        local cells: {Node} = {badge}
        if read_only then
            if card then
                table.insert(cells, {kind = "text" :: "text", text = string.format("%03d", card.id), font = "mono" :: Font,
                    color = if issues[slot] then "danger" :: Color else "muted" :: Color})
                table.insert(cells, {kind = "text" :: "text", width = "fill" :: Size,
                    text = labels.name(info, card.id) .. (if info then " — " .. labels.effect(info.effect) else ""),
                    color = if card.enabled then nil else "muted" :: Color, strikethrough = not card.enabled})
            else table.insert(cells, {kind = "text" :: "text", text = t("patch-card4-none"), color = "muted" :: Color, width = "fill" :: Size}) end
        else
            table.insert(cells, {kind = "select" :: "select", id = "patch_cards.slot:" .. tostring(slot), width = "fill" :: Size,
                value = if card then tostring(card.id) else "none", choices = choices[slot], content_padding = {5, 10, 5, 10},
                appearance = if issues[slot] then {text_color = "danger" :: Color} else nil})
            table.insert(cells, ui.toggle("patch_cards.toggle:" .. tostring(slot), "ON", card ~= nil and card.enabled,
                {0x29 / 255, 0xa1 / 255, 0x21 / 255, 1} :: Color, card ~= nil))
        end
        local content: {Node} = {ui.row(cells, if read_only and card then 8 else 10)}
        local bugs = if info then labels.bugs(info.bugs) else nil
        if bugs then table.insert(content, ui.row({{kind = "space" :: "space", width = 44},
            {kind = "text" :: "text", text = bugs, color = {0xb5 / 255, 0x5a / 255, 0xde / 255, 1} :: Color}}, 0)) end
        table.insert(rows, {kind = "container" :: "container", width = "fill" :: Size, padding = {8, 10, 8, 10}, tone = {row = slot} :: Tone,
            appearance = if issues[slot] then {text_color = "danger" :: Color} else nil,
            tooltip = if issues[slot] then popover.detail(nil, nil, nil, {issues[slot]}) else nil,
            child = ui.column(content, 2)})
    end
    local list = ui.column(rows, 3)
    if read_only then return {kind = "container" :: "container", width = "fill" :: Size, tone = "card" :: Tone, child = list} end
    local header: Node = {kind = "container" :: "container", width = "fill" :: Size, padding = {8, 12, 8, 12}, child = ui.row({
        {kind = "text" :: "text", text = t("save-tab-patch-cards")}, ui.caption(t("patch-card-edit-count", {count = count}), false),
        {kind = "space" :: "space", width = "fill" :: Size}, ui.clear_button("patch_cards.clear", t("save-edit-clear")),
    }, 8)}
    return ui.pane(header, list)
end
return {view = view, viewer = function(value: cards.Model, assets: controller.Assets): Node return view(value, assets, true) end}
