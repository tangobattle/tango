--!strict
local import = require("@bn-common/gamemode/legacy_replay")
local cartridges = {
    bn5 = {[0] = {code = "BRBE", crc32 = 0xa73e83a4, size = 0x800000}, [1] = {code = "BRKE", crc32 = 0xa552f683, size = 0x800000}},
    exe5 = {[0] = {code = "BRBJ", crc32 = 0xc73f23c0, size = 0x800000}, [1] = {code = "BRKJ", crc32 = 0x16842635, size = 0x800000}},
}

return function(battle_type: number): (LegacyReplay, buffer) -> ReplayImport?
    return function(legacy: LegacyReplay, rom: buffer): ReplayImport?
        -- Revision 1 uses the single-confirm menu path implemented by these
        -- hooks. Older recordings used a different boot sequence.
        return import(legacy, rom, battle_type, 1, cartridges)
    end
end
