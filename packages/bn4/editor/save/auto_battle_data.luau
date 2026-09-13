--!strict
local storage = require("@bn-common/save/auto_battle_data")
local save = require("./init")
export type Model = storage.Model
export type Assets = storage.Assets
local function open(bytes: buffer): Model
    return storage.open(bytes, {num_chips = 350, primary = save.START + 0x6f50,
        secondary = save.START + 0x1bb0, materialized = save.START + 0x5064})
end
return {open = open}
