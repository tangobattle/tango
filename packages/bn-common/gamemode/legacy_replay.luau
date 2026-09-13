--!strict
export type Cartridge = {code: string, crc32: number, size: number}
export type Cartridges = {[string]: {[number]: Cartridge}}

-- The historical GBA recordings carry a native simulation revision and an
-- exact cartridge identity. Reject unknown behavior instead of guessing.
local function import(legacy: LegacyReplay, rom: buffer, match_type: number, sim_version: number,
    cartridges: Cartridges): ReplayImport?
    if legacy.match_type ~= match_type or legacy.match_subtype ~= 0 then return nil end
    if legacy.engine.name ~= "mgba" or legacy.engine.revision ~= 2 then return nil end
    if legacy.game.sim_version ~= sim_version or legacy.game.patch ~= nil then return nil end
    local family = cartridges[legacy.game.rom_family]
    local cartridge = if family then family[legacy.game.rom_variant] else nil
    if not cartridge or buffer.len(rom) ~= cartridge.size or cartridge.size < 0xb0 then return nil end
    if buffer.readstring(rom, 0xac, 4) ~= cartridge.code or tango.crc32(rom) ~= cartridge.crc32 then return nil end
    return {disable_bgm = false, options = {}}
end

return import
