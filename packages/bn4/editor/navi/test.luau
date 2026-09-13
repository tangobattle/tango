--!strict
local effects = require("./effects")
local stats = require("./stats")
local reference = require("../save/fixtures/catalog")
local save = require("../save")
local model = require("../save/model")
local catalog = require("../patch_cards/catalog")

return function()
    for id = 0, 187 do
        local expected, actual = reference.parts[id + 1], effects.for_part(id)
        assert(#actual == #expected)
        for index, effect in actual do
            local name = string.gsub(effect.kind, "_", "")
            local text = name .. (if effect.value then "(" .. tostring(effect.value) .. ")" else "")
            assert(text == expected[index], tostring(id))
        end
    end
    for group = 1, 10 do
        local expected, actual = reference.bugs[group], effects.for_group(group)
        assert(#actual == #expected)
        for index, bug in actual do assert(string.gsub(bug, "_", "") == expected[index]) end
    end
    for _, class in {"standard", "mega", "giga", "program_advance", "none"} do
        assert(stats.max_copies({class = class, dark = true}) == 0)
        assert(stats.max_copies({class = class, dark = false}) == (if class == "standard" then 4
            elseif class == "mega" or class == "giga" then 1 else 0))
    end

    local bytes = save.from_wram(tango.read_file("editor/save/templates/light_hp_1000_rs_us.raw"), {region = "US", variant = "RedSun"})
    local value = model.open(bytes)
    for slot = 1, 25 do assert(value.set_navicust_part(slot, nil)) end
    for slot = 1, 6 do assert(value.set_patch_card(slot, nil)) end
    assert(value.clear_materialized())
    assert(value.set_navicust_part(1, {id = 164, col = 2, row = 2, rot = 0, compressed = false}))
    assert(value.set_patch_card(1, {id = 7, enabled = true}))
    -- Two cells referencing the same placement still contribute only once.
    buffer.writeu8(bytes, save.START + 0x4540, 1)
    buffer.writeu8(bytes, save.START + 0x4541, 1)
    local assets = {navicust_command_line = 2,
        navicust_part = function(id: number): {effects: {effects.Effect}}? return {effects = effects.for_part(id)} end,
        patch_card = function(id: number): catalog.Card? return catalog.get(id, "US") end}
    assert(stats.derive(value, assets).max_hp == 1300)
    local original = assets.patch_card
    assets.patch_card = function(id: number): catalog.Card?
        local card = original(id)
        if not card then return nil end
        local copy = table.clone(card)
        copy.effect = {kind = "max_hp", value = 750}
        return copy
    end
    assert(stats.derive(value, assets).max_hp == 1800)
    assets.navicust_part = function(_id: number): {effects: {effects.Effect}}? return {effects = {{kind = "max_hp", value = 125}}} end
    assert(stats.derive(value, assets).max_hp == 1875)
    assert(value.set_patch_card(1, {id = 7, enabled = false}))
    assert(stats.derive(value, assets).max_hp == 1125)
end
