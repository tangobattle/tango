--!strict
local text_utils = require("./text")
local function lines(text: string): {string}
    local result: {string} = {}
    local start = 1
    while start <= #text do
        local finish = string.find(text, "\n", start, true)
        local line = string.sub(text, start, if finish then finish - 1 else #text)
        if finish and string.sub(line, -1) == "\r" then line = string.sub(line, 1, -2) end
        table.insert(result, line)
        if not finish then break end
        start = finish + 1
    end
    return result
end

local function wrap(text: string): {string}
    local normalized: {string} = {}
    for _, cp in utf8.codes(text) do
        local space = text_utils.is_space(cp)
        table.insert(normalized, if space then " " else utf8.char(cp))
    end
    local result: {string} = {}
    local current = ""
    for word in string.gmatch(table.concat(normalized), "%S+") do
        local length = utf8.len(word) :: number
        if length > 36 then
            if current ~= "" then table.insert(result, current); current = "" end
            local start = 1
            while length > 36 do
                local finish = utf8.offset(word, 37, start) :: number
                table.insert(result, string.sub(word, start, finish - 1))
                start = finish
                length -= 36
            end
            current = string.sub(word, start)
        else
            if current ~= "" and (utf8.len(current) :: number) + 1 + length > 36 then
                table.insert(result, current)
                current = ""
            end
            current ..= (if current == "" then "" else " ") .. word
        end
    end
    if current ~= "" then table.insert(result, current) end
    return result
end

local function detail(image: Raster?, description: string?, accent: Color?, issues: {string}): Tooltip?
    if not image and description == nil and #issues == 0 then return nil end
    local children: {Node} = {}
    if image then
        table.insert(children, {kind = "image" :: "image", image = image, width = image.width * 2, height = image.height * 2, nearest = true} :: Node)
    end
    for _, line in lines(description or "") do table.insert(children, {kind = "text" :: "text", text = line, size = "caption" :: TextSize} :: Node) end
    for _, issue in issues do
        for _, line in lines(issue) do
            local warning: {Node} = {}
            for _, wrapped in wrap(line) do table.insert(warning, {kind = "text" :: "text", text = wrapped, size = "caption" :: TextSize} :: Node) end
            local row_children: {Node} = {
                {kind = "text", text = "•", color = "danger"},
                {kind = "column", spacing = 2, children = warning},
            }
            table.insert(children, {kind = "row", spacing = 6, children = row_children} :: Node)
        end
    end
    return {position = "follow_cursor" :: TooltipPosition, gap = 8, content = {
        kind = "container" :: "container", padding = 8,
        appearance = {background = accent or {0, 0, 0, 0.85}, text_color = {1, 1, 1, 1},
            border_color = {1, 1, 1, 0.2}, border_width = 1, radius = 4},
        child = {kind = "column" :: "column", spacing = 6, children = children},
    }}
end

return {detail = detail, lines = lines, wrap = wrap}
