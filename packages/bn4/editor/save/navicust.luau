--!strict
local storage = require("@bn-common/save/navicust")
local save = require("./init")
export type Model = storage.Model
export type Assets = storage.Assets
local function open(bytes: buffer): Model
    return storage.open(bytes, {count = 25, num_parts = 188, parts = save.START + 0x4564,
        grid = save.START + 0x4540, grid_bytes = 0x24, colors = save.START + 0x190, board_size = 5, bitmap_size = 5})
end
return {open = open}
