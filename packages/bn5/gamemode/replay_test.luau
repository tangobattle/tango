--!strict
local import = require('./replay')
local detect = require('../rom/detect')

return function()
    local legacy: LegacyReplay = {
        game = {rom_family = 'bn5', rom_variant = 0, sim_version = 1},
        match_type = 0, match_subtype = 0, engine = {name = 'mgba', revision = 2},
    }
    assert(import(0)(legacy, buffer.create(0)) == nil)
    local size = tango.input_size('rom')
    if not size or size ~= 0x800000 then return end
    local rom = tango.read_input('rom', 0, size)
    if not detect(rom) then return end
    local code = buffer.readstring(rom, 0xac, 4)
    legacy.game.rom_family = if string.sub(code, 4, 4) == 'J' then 'exe5' else 'bn5'
    legacy.game.rom_variant = if string.sub(code, 3, 3) == 'B' then 0 else 1
    for mode = 0, 1 do
        legacy.match_type = mode
        local result = import(mode)(legacy, rom)
        assert(result ~= nil and result.disable_bgm == false)
        assert(import((mode + 1) % 2)(legacy, rom) == nil)
    end
    legacy.game.sim_version = 0
    assert(import(1)(legacy, rom) == nil)
    legacy.game.sim_version = 1
    legacy.match_subtype = 1
    assert(import(1)(legacy, rom) == nil)
    legacy.match_subtype = 0
    legacy.game.patch = {name = 'unknown', version = '1.0.0'}
    assert(import(1)(legacy, rom) == nil)
    legacy.game.patch = nil
    legacy.engine.revision = 3
    assert(import(1)(legacy, rom) == nil)
    legacy.engine.revision = 2
    buffer.writeu8(rom, size - 1, bit32.bxor(buffer.readu8(rom, size - 1), 1))
    assert(import(1)(legacy, rom) == nil)
end
