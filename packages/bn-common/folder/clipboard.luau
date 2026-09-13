--!strict
local display = require("./display")
local catalog = require("./catalog")
local clipboard = require("@editor-common/clipboard")
local ui = require("@editor-common/ui")

local function copy(view: display.View, assets: catalog.Assets, grouped: boolean): Effect
    local text: {string}, html: {string} = {}, {"<table><tbody>"}
    local icons: {[number]: string} = {}
    for _, item in display.items(view, grouped) do
        local chip = item.chip
        local info = if chip then assets.chip(chip.id) else nil
        local name = if info and info.name then info.name else if chip then "???" else "---"
        local suffix = if chip then (if item.regular then "[REG]" else "")
            .. (if item.tag1 then "[TAG1]" else "") .. (if item.tag2 then "[TAG2]" else "") else ""
        local code = if chip then (if chip.code == 26 then "*" else string.char(65 + chip.code)) else ""
        table.insert(text, (if grouped then tostring(item.count) .. "\t" else "") .. name
            .. (if chip then "\t" .. code .. (if suffix ~= "" then "\t" .. suffix else "") else "") .. "\n")
        table.insert(html, "<tr>" .. (if grouped then "<td>" .. tostring(item.count) .. "</td>" else "") .. "<td>")
        if chip then
            if not icons[chip.id] and info then
                local icon = info.icon()
                if icon then
                    if icon.width >= 15 and icon.height >= 15 then icon = ui.crop(icon, 1, 1, 14, 14) end
                    icons[chip.id] = clipboard.png_data_url(icon)
                end
            end
            if icons[chip.id] then table.insert(html, '<img alt="" width="14" height="14" src="' .. icons[chip.id] .. '"> ') end
        end
        table.insert(html, clipboard.html_escape(name) .. "</td>")
        if chip then table.insert(html, "<td>" .. clipboard.html_escape(code) .. "</td><td>" .. suffix .. "</td>") end
        table.insert(html, "</tr>")
    end
    table.insert(html, "</tbody></table>")
    return {kind = "copy_html", text = table.concat(text), html = table.concat(html)}
end

return {copy = copy}
