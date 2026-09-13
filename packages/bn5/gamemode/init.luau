--!strict
local detect_rom = require("../rom/detect")
local rom_offsets = require("./offsets")
local install_hooks = require("./hooks")
local import_replay = require("./replay")

return function(battle_type: number): GameMode
    return {
        platform = "gba",
        detect_rom = detect_rom,
        import_replay = import_replay(battle_type),
        setup = function(context: GameModeContext, game: Game)
            local header = tango.read_input("rom", 0, 0xc0)
            assert(detect_rom(header), "unsupported BN5 ROM")
            local offsets = rom_offsets[buffer.readstring(header, 0xac, 4)]
            assert(offsets)
            assert(context.options.battle_type == nil, "select single or triple as the gamemode")
            install_hooks(context, game, offsets, battle_type)
        end,
    }
end
