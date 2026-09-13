--!strict
local clipboard = require("./clipboard")
local catalog = require("./catalog")
local display = require("./display")
local rules = require("./rules")

local function test()
    local chips: {[number]: rules.Chip} = {[1] = {id = 1, code = 0}, [2] = {id = 9, code = 26}, [3] = {id = 1, code = 0}}
    local view: display.View = {size = 5, chip = function(slot: number): rules.Chip? return chips[slot] end,
        regular = function(): number? return 1 end, tags = function(): {number}? return {1, 3} end}
    local assets: catalog.Assets = {num_chips = 2, chips_have_mb = true,
        chip_is_legal = function(_id: number): boolean return true end,
        element_icon = function(_id: number): Raster? return nil end,
        chip = function(id: number): catalog.Chip?
            if id ~= 1 then return nil end
            return {name = "<&\"'保存>", codes = {0}, class = "standard", mb = 1, element = 0, attack = 1,
                icon = function(): Raster? return nil end, image = function(): Raster? return nil end}
        end}
    local grouped = clipboard.copy(view, assets, true)
    assert(grouped.kind == "copy_html")
    assert(grouped.text == "2\t<&\"'保存>\tA\t[REG][TAG1][TAG2]\n1\t???\t*\n2\t---\n")
    assert(grouped.html == '<table><tbody><tr><td>2</td><td>&lt;&amp;&quot;&#39;保存&gt;</td><td>A</td><td>[REG][TAG1][TAG2]</td></tr>'
        .. '<tr><td>1</td><td>???</td><td>*</td><td></td></tr><tr><td>2</td><td>---</td></tr></tbody></table>')
    local separate = clipboard.copy(view, assets, false)
    assert(separate.kind == "copy_html")
    assert(separate.text == "<&\"'保存>\tA\t[REG][TAG1]\n???\t*\n<&\"'保存>\tA\t[TAG2]\n---\n---\n")
    table.clear(chips)
    local empty = clipboard.copy(view, assets, true)
    assert(empty.kind == "copy_html" and empty.text == "5\t---\n")
    assert(empty.html == "<table><tbody><tr><td>5</td><td>---</td></tr></tbody></table>")
end

return {test = test}
