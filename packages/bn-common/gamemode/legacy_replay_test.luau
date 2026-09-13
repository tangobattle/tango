--!strict
local import = require("./legacy_replay")

return function()
    local rom = buffer.create(0xc0)
    buffer.writestring(rom, 0xac, "TEST")
    local cartridges = {test = {[0] = {code = "TEST", size = buffer.len(rom), crc32 = tango.crc32(rom)}}}
    local legacy: LegacyReplay = {game = {rom_family = "test", rom_variant = 0, sim_version = 1},
        match_type = 0, match_subtype = 0, engine = {name = "mgba", revision = 2}}
    local result = import(legacy, rom, 0, 1, cartridges)
    assert(result and not result.disable_bgm and next(result.options or {}) == nil)
    assert(import(legacy, buffer.create(0), 0, 1, cartridges) == nil)
    assert(import(legacy, rom, 1, 1, cartridges) == nil)
    assert(import(legacy, rom, 0, 0, cartridges) == nil)
    legacy.match_subtype = 1
    assert(import(legacy, rom, 0, 1, cartridges) == nil)
    legacy.match_subtype = 0
    legacy.game.rom_family = "unknown"
    assert(import(legacy, rom, 0, 1, cartridges) == nil)
    legacy.game.rom_family = "test"
    legacy.game.rom_variant = 1
    assert(import(legacy, rom, 0, 1, cartridges) == nil)
    legacy.game.rom_variant = 0
    legacy.game.patch = {name = "unknown", version = "1.0.0"}
    assert(import(legacy, rom, 0, 1, cartridges) == nil)
    legacy.game.patch = nil
    legacy.engine.name = "unknown"
    assert(import(legacy, rom, 0, 1, cartridges) == nil)
    legacy.engine.name, legacy.engine.revision = "mgba", 1
    assert(import(legacy, rom, 0, 1, cartridges) == nil)
    legacy.engine.revision = 2
    buffer.writeu8(rom, 0xac, 0)
    assert(import(legacy, rom, 0, 1, cartridges) == nil)
    buffer.writestring(rom, 0xac, "TEST")
    buffer.writeu8(rom, 0xbf, 1)
    assert(import(legacy, rom, 0, 1, cartridges) == nil)
end
