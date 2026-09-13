--!strict
-- Compatibility with the retired SIO-based native implementation. Keep legacy
-- codes here; ordinary sessions select the individual gamemode exports.
local import = require("@bn-common/gamemode/legacy_replay")
local cartridges = {
    bn6 = {[0] = {code = "BR5E", crc32 = 0x79452182, size = 0x800000}, [1] = {code = "BR6E", crc32 = 0xdee6f2a9, size = 0x800000}},
    exe6 = {[0] = {code = "BR5J", crc32 = 0x6285918a, size = 0x800000}, [1] = {code = "BR6J", crc32 = 0x2dfb603e, size = 0x800000}},
}

return function(battle_type: number): (LegacyReplay, buffer) -> ReplayImport?
    return function(legacy: LegacyReplay, rom: buffer): ReplayImport?
        return import(legacy, rom, battle_type, 0, cartridges)
    end
end
