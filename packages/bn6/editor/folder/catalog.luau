--!strict
local catalog = require("@bn-common/folder/catalog")
local rom = require("../rom")

local function catalog_assets(assets: rom.Assets): catalog.Assets
    return {num_chips = 411, chips_have_mb = true, chip_is_legal = assets.chip_is_legal,
        chip = function(id: number): catalog.Chip?
            local chip = assets.chip(id)
            if not chip then return nil end
            return {name = chip.name, codes = chip.codes, class = chip.class, dark = false, mb = chip.mb,
                description = chip.description, element = chip.element, attack = chip.attack,
                icon = function(): Raster? return chip.icon() end,
                image = function(): Raster? return chip.image() end}
        end,
        element_icon = assets.element_icon}
end

return catalog_assets
