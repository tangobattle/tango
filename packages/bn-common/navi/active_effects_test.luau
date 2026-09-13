--!strict
local active = require("./active_effects")

return function()
    local view: active.View = {
        materialized = function(): {number} return {1, 1, 0, 0, 3, 3, 0, 0, 0} end,
        navicust_part = function(slot: number): {id: number}?
            return if slot == 1 or slot == 3 then {id = slot * 4} else nil
        end,
        patch_count = function(): number return 3 end,
        patch_card = function(slot: number): {id: number, enabled: boolean}
            return {id = slot, enabled = slot ~= 2}
        end,
    }
    local assets: active.Assets = {
        navicust_part = function(id: number)
            return {effects = {{kind = "max_hp", value = id * 10}}}
        end,
        patch_card = function(id: number)
            return {effects = {{id = id, kind = "mega_folder_plus", parameter = id, ability = false, debuff = false}}}
        end,
    }
    local parts = active.parts(view, assets, 3, nil)
    assert(#parts == 2 and parts[1].value == 40 and parts[2].value == 120)
    parts = active.parts(view, assets, 3, 0)
    assert(#parts == 1 and parts[1].value == 40)
    parts = active.parts(view, assets, 3, 1)
    assert(#parts == 1 and parts[1].value == 120)
    local cards = active.patch_cards(view, assets)
    assert(#cards == 2 and cards[1].parameter == 1 and cards[2].parameter == 3)
end
