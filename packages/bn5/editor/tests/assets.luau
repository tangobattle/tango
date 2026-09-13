--!strict
local rom = require("../rom")
local offsets = require("../rom/offsets")
local parts = require("../rom/parts")

return function(game: {region: string, variant: string}): rom.Assets
    local definition = offsets.definitions[(if game.variant == "Protoman" then "BRB" else "BRK") ..
        (if game.region == "JP" then "J" else "E")]
    local raster: Raster = {width = 16, height = 16, rgba = buffer.create(16 * 16 * 4)}
    return {
        definition = definition, navicust_command_line = 2,
        chip_is_legal = function(id: number): boolean return id >= 1 and id <= 10 end,
        chip = function(id: number): rom.Chip?
            if id < 1 or id > 10 then return nil end
            return {id = id, name = "Chip " .. tostring(id), description = "Description", codes = {0, 26},
                class = "standard", dark = false, legal = true, element = 0, mb = 10, attack = 20, order = id,
                icon = function(): Raster return raster end, image = function(): Raster return raster end}
        end,
        element_icon = function(_id: number): Raster? return raster end,
        navicust_part = function(id: number): parts.Part?
            if id ~= 4 and id ~= 8 then return nil end
            return {id = id, name = "Part " .. tostring(id), description = "Description", color = "white", color_id = 1,
                solid = true, effect_group = 0, effects = {}, bugs = {}, bitmap = function(compressed: boolean): {boolean}?
                    local result: {boolean} = {}
                    for index = 1, 25 do result[index] = index == 13 or (not compressed and index == 8) end
                    return result
                end}
        end,
        patch_card = function(id: number): parts.Card?
            if id ~= 1 and id ~= 2 then return nil end
            return {id = id, name = "Card " .. tostring(id), mb = if id == 1 then 50 else 40, effects = {}}
        end,
        navi = function(id: number): parts.Navi?
            for _, row in definition.navi_order do
                if table.find(row, id) then return {id = id, name = "Navi " .. tostring(id), emblem = raster} end
            end
            return nil
        end,
    }
end
