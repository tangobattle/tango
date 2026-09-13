--!strict
local edit = require("./edit")
local rules = require("./rules")

local function test()
    local chips: {[number]: rules.Chip?} = {[1] = {id = 1, code = 0}, [2] = {id = 2, code = 0}, [4] = {id = 3, code = 0}}
    local regular: number? = 1
    local tags: {number}? = nil
    local mirrors = 0
    local view: edit.View = {
        size = 5, chip = function(slot: number): rules.Chip? return chips[slot] end,
        regular = function(): number? return regular end, tags = function(): {number}? return tags end,
        set_chip = function(slot: number, chip: rules.Chip?): boolean chips[slot] = chip; return true end,
        set_regular = function(slot: number?): boolean regular = slot; return true end,
        set_tags = function(pair: {number}?): boolean tags = pair; return true end,
        rebuild_anticheat = function() mirrors += 1 end,
    }
    local assets: rules.Assets = {chip = function(id: number): rules.ChipInfo?
        return {codes = {0}, class = "mega", mb = id * 10}
    end, chip_is_legal = function(_id: number): boolean return true end}
    local limits: rules.Limits = {mega_limit = 0, reg_memory = 15, tag_memory = 30,
        max_copies = function(_chip: rules.ChipInfo): number return 0 end}
    local function apply(action: edit.Action) edit.apply(view, assets, limits, action) end
    local function id(slot: number): number? local chip = chips[slot]; return if chip then chip.id else nil end
    -- Add at the top and consume the first gap, preserving later holes.
    apply({kind = "add", chip = {id = 4, code = 0}})
    assert(id(1) == 4 and id(2) == 1 and id(3) == 2 and id(4) == 3 and id(5) == nil and regular == 2)
    assert(mirrors == 1) -- class/copy caps do not reject the edit
    apply({kind = "regular", slot = 1})
    assert(regular == 2 and mirrors == 1) -- 40MB does not fit
    assert(not edit.regular_allowed(view, assets, limits, regular, {}, 1))
    assert(edit.regular_allowed(view, assets, limits, 1, {}, 1)) -- clearing remains possible
    apply({kind = "tags", pair = {2, 3}})
    assert(tags and tags[1] == 2 and tags[2] == 3)
    apply({kind = "tags", pair = {1, 4}})
    assert(tags and tags[1] == 2 and tags[2] == 3 and mirrors == 2)
    assert(edit.tag_allowed(view, assets, limits, nil, {2}, 3))
    assert(not edit.tag_allowed(view, assets, limits, nil, {2}, 4))
    assert(not edit.tag_allowed(view, assets, limits, 3, {}, 3))
    assert(not edit.tag_allowed(view, assets, limits, nil, {2, 3}, 4))
    assert(edit.tag_allowed(view, assets, limits, nil, {1, 4}, 1))
    apply({kind = "move", from = 1, to = 3})
    assert(id(1) == 1 and id(2) == 2 and id(3) == 4 and id(4) == 3 and regular == 1)
    assert(tags and tags[1] == 1 and tags[2] == 2)
    assert(edit.drop_target(view, 1, 5) == 4 and edit.drop_target(view, 5, 1) == nil)
    apply({kind = "move", from = 1, to = 5}) -- model refuses a direct move into a gap
    assert(id(1) == 1 and mirrors == 3)
    apply({kind = "remove", slot = 1})
    assert(id(1) == 2 and id(2) == 4 and id(3) == 3 and id(4) == nil and regular == nil and tags == nil)
    local staged = edit.toggle_tag({}, 3)
    assert(#staged == 1 and staged[1] == 3)
    staged = edit.toggle_tag(staged, 1)
    assert(#staged == 2 and staged[1] == 3 and staged[2] == 1)
    assert(#edit.toggle_tag(staged, 2) == 2)
    staged = edit.remap_staged(staged, function(slot: number): number? return if slot == 1 then nil else slot - 1 end)
    assert(#staged == 1 and staged[1] == 2)
    staged = edit.remap_staged(staged, function(slot: number): number return edit.reorder_index(slot, 2, 1) end)
    assert(staged[1] == 1 and #edit.toggle_tag(staged, 1) == 0)
    apply({kind = "clear"})
    for slot = 1, 5 do assert(chips[slot] == nil) end
    assert(regular == nil and tags == nil and mirrors == 5)
    for _, slot in {0, -1, 6, 0.5, 0/0, math.huge} do apply({kind = "remove", slot = slot}) end
    assert(mirrors == 5)
    for _, slot in {0, 6, 0.5, 0/0} do
        assert(not edit.regular_allowed(view, assets, limits, nil, {}, slot))
        assert(not edit.tag_allowed(view, assets, limits, nil, {}, slot))
    end
end

return {test = test}
