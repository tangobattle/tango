--!strict
local folders = require("@bn-common/save/folders")
local save = require("./init")
export type Model = folders.Model
local function open(bytes: buffer): Model
    local function address(offset: number): number return save.START + offset end
    return folders.open(bytes, {count = 3, chips = address(0x262c), equipped = address(0x2132),
        regular = address(0x214d), active_regular = address(0x214c), pack = address(0x26e4), pack_chips = 321,
        anticheat_source = address(0x670), anticheat_target = address(0x58fc), anticheat_size = 0x200,
        anticheat_key = if save.info(bytes).variant == "RedSun" then 0x64 else 0x47})
end
return {open = open}
