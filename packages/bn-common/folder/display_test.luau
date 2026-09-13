--!strict
local display = require("./display")
local rules = require("./rules")

local function test()
    local chips: {[number]: rules.Chip} = {[1] = {id = 4, code = 0}, [2] = {id = 7, code = 26},
        [3] = {id = 4, code = 0}, [5] = {id = 4, code = 1}, [6] = {id = 4, code = 0}}
    local view: display.View = {size = 7, chip = function(slot: number): rules.Chip? return chips[slot] end,
        regular = function(): number? return 1 end, tags = function(): {number}? return {3, 6} end}
    local grouped = display.items(view, true)
    assert(#grouped == 4)
    assert((grouped[1].chip :: rules.Chip).id == 4 and grouped[1].count == 3)
    assert(grouped[1].regular and grouped[1].tag1 and grouped[1].tag2)
    assert(table.concat(grouped[1].slots, ",") == "1,3,6")
    assert((grouped[2].chip :: rules.Chip).id == 7)
    assert(grouped[3].chip == nil and grouped[3].count == 2)
    assert((grouped[4].chip :: rules.Chip).code == 1)
    local separate = display.items(view, false)
    assert(#separate == 7 and separate[4].chip == nil and separate[7].chip == nil)
    assert(separate[1].regular and not separate[1].tag1 and separate[3].tag1 and separate[6].tag2)
    for index, item in separate do assert(item.count == 1 and #item.slots == 1 and item.slots[1] == index) end
    local assets: rules.Assets = {chip_is_legal = function(id: number): boolean return id == 7 end,
        chip = function(_id: number): rules.ChipInfo? return {name = "Test", codes = {0, 26}, class = "mega", mb = 30} end}
    local limits: rules.Limits = {max_copies = function(_info: rules.ChipInfo): number return 2 end,
        mega_limit = 4, reg_memory = 20, tag_memory = 40}
    local issues = display.issues(grouped[1], view, assets, rules.scan(view, assets), limits)
    -- Repeated slot issues collapse, but Regular and combined Tag memory each
    -- remain represented once, in the order their first contributing slot occurs.
    assert(#issues == 5)
    assert(issues[1].kind == "chip_illegal_for_game" and issues[2].kind == "too_many_copies_of_chip")
    assert(issues[3].kind == "too_many_mega_chips" and issues[4].kind == "regular_chip_exceeds_memory")
    assert(issues[5].kind == "tag_chips_exceed_memory")
end

return {test = test}
