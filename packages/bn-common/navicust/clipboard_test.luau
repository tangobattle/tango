--!strict
local clipboard = require("./clipboard")
local types = require("./types")

local function test()
    local definitions: {[number]: types.Info} = {
        [1] = {name = "Solid", solid = true, bitmap = function(_small: boolean): types.Bitmap? return nil end},
        [2] = {name = "Plus", solid = false, bitmap = function(_small: boolean): types.Bitmap? return nil end},
        [3] = {solid = true, bitmap = function(_small: boolean): types.Bitmap? return nil end},
    }
    local ids = {2, 1, 9, 3}
    local view: types.View = {count = function(): number return #ids end,
        part = function(slot: number): types.Placement? return {id = ids[slot], col = 0, row = 0, rot = 0, compressed = false} end,
        set_part = function(_slot: number, _part: types.Placement?): boolean return false end,
        rebuild = function() end, color_bar = function(): {string} return {} end}
    local assets: types.Assets = {num_parts = 4, part = function(id: number): types.Info? return definitions[id] end,
        layout = {cols = 7, rows = 7, command_line = 3, out_of_bounds = true, background = {0, 0, 0, 1}}}
    assert(clipboard.text(view, assets, nil) == "Solid\tPlus\n#3\t\n")
    assert(clipboard.text(view, assets, "Style") == "Style\nSolid\tPlus\n#3\t\n")
    table.clear(ids)
    assert(clipboard.text(view, assets, nil) == "")
    assert(clipboard.text(view, assets, "Style") == "Style\n")
end

return {test = test}
