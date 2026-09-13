--!strict
local types = require("./types")
local shape = require("./shape")
local catalog = require("./catalog")
local edit = require("./edit")
local controller = require("./state")

local function test()
    local plain: types.Bitmap = {size = 3, cells = {false, true, false, false, true, true, false, false, false}}
    local compressed: types.Bitmap = {size = 3, cells = {false, false, false, false, true, true, false, false, false}}
    local function info(name: string, color: string): types.Info
        return {name = name, color = color, solid = true,
            bitmap = function(small: boolean): types.Bitmap return if small then compressed else plain end}
    end
    local definitions: {[number]: types.Info} = {[1] = info("Éclair", "red"), [2] = info("Beta", "white")}
    local assets: types.Assets = {num_parts = 3, part = function(id: number): types.Info? return definitions[id] end,
        layout = {cols = 7, rows = 7, command_line = 3, out_of_bounds = true, background = {0, 0, 0, 1}}}
    local parts: {[number]: types.Placement?} = {}
    local rebuilt = 0
    local view: types.View = {count = function(): number return 12 end,
        part = function(slot: number): types.Placement? return parts[slot] end,
        set_part = function(slot: number, part: types.Placement?): boolean parts[slot] = part; return true end,
        rebuild = function() rebuilt += 1 end, color_bar = function(): {string} return {} end}
    local function placed(slot: number): types.Placement
        local part = parts[slot]
        assert(part)
        return part
    end
    local turned = shape.offsets(plain, 1)
    assert(turned[1].col == 1 and turned[1].row == 0)
    assert(turned[3].col == 0 and turned[3].row == 1)
    assert(shape.compressible(definitions[1]))
    local scratch: ViewState = {}
    controller.apply(scratch, view, assets, {kind = "pick", id = 1})
    local held = controller.read(scratch).held :: types.Held
    assert(held.compressed and held.rot == 0)
    local grid = shape.materialize(view, assets)
    assert(not (shape.ghost(held, {col = 0, row = 0}, assets, grid) :: shape.Ghost).legal)
    assert(not (shape.ghost(held, {col = 2, row = 0}, assets, grid) :: shape.Ghost).legal)
    assert(not (shape.ghost(held, {col = 6, row = 2}, assets, grid) :: shape.Ghost).legal)
    assert((shape.ghost(held, {col = 0, row = 2}, assets, grid) :: shape.Ghost).legal)
    controller.apply(scratch, view, assets, {kind = "cell", col = 2, row = 2})
    assert(placed(1).col == 2 and rebuilt == 1 and controller.read(scratch).held == nil)
    -- Picking the outer cell retains its offset as the part rotates.
    controller.apply(scratch, view, assets, {kind = "cell", col = 3, row = 2})
    held = controller.read(scratch).held :: types.Held
    assert(held.grab_col == 1 and held.grab_row == 0 and parts[1] == nil)
    controller.apply(scratch, view, assets, {kind = "rotate_held"})
    held = controller.read(scratch).held :: types.Held
    assert(held.rot == 1 and held.grab_col == 0 and held.grab_row == 1)
    local rot, small = controller.orientation(scratch, 1)
    assert(rot == 1 and small)
    controller.apply(scratch, view, assets, {kind = "cell", col = 4, row = 3})
    assert(placed(1).col == 4 and placed(1).row == 2 and placed(1).rot == 1)
    controller.apply(scratch, view, assets, {kind = "cell", col = 4, row = 3})
    controller.apply(scratch, view, assets, {kind = "compress", id = 1})
    held = controller.read(scratch).held :: types.Held
    assert(not held.compressed and held.grab_col == 0 and held.grab_row == 0)
    controller.apply(scratch, view, assets, {kind = "cell", col = 3, row = 3})
    controller.apply(scratch, view, assets, {kind = "pick", id = 2})
    controller.apply(scratch, view, assets, {kind = "cell", col = 3, row = 3})
    assert(controller.read(scratch).held ~= nil and parts[2] == nil) -- collision leaves the part held
    controller.apply(scratch, view, assets, {kind = "cell", col = 1, row = 1})
    assert(placed(2).id == 2)
    assert(edit.apply(view, {kind = "remove", slot = 1}))
    assert(placed(1).id == 2 and parts[2] == nil)
    assert(not edit.apply(view, {kind = "remove", slot = 13}))
    controller.apply(scratch, view, assets, {kind = "clear"})
    assert(next(parts) == nil and controller.read(scratch).held == nil)
    for _ = 1, 9 do assert(edit.apply(view, {kind = "add", part = {id = 1, col = 3, row = 3, rot = 0, compressed = true}})) end
    assert(not edit.apply(view, {kind = "add", part = {id = 1, col = 3, row = 3, rot = 0, compressed = true}}))
    controller.apply(scratch, view, assets, {kind = "pick", id = 1})
    assert(controller.read(scratch).held == nil)
    grid = shape.materialize(view, assets)
    assert(grid[3 * 7 + 3 + 1] == 9) -- native materialization keeps the last overlapping slot
    local entries = catalog.entries(assets, "color", "")
    assert(entries[1].id == 2 and entries[2].id == 1)
    entries = catalog.entries(assets, "name", "ÉC")
    assert(#entries == 1 and entries[1].id == 1)
end

return {test = test}
