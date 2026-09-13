--!strict
local rng = require("@bn-common/gamemode/rng")
local lifecycle = require("@bn-common/gamemode/lifecycle")

local BATTLE_TICK = 0x02034880 + 0x60
local SUBMENU = 0x02009a30
local RNG1 = 0x02001120
local RNG2 = 0x020013f0

return function(context: GameModeContext, game: Game, offsets: {[string]: number}, battle_type: number)
    local rng1 = rng(context.seed, context.player, 1)
    local rng2 = rng(context.seed, context.player, 2)
    local function pc(value: number)
        game.write_register("thumb_pc", value)
    end
    local function write(address: number, length: number, value: number)
        local bytes = buffer.create(length)
        if length == 1 then buffer.writeu8(bytes, 0, value)
        elseif length == 2 then buffer.writeu16(bytes, 0, value)
        else buffer.writeu32(bytes, 0, value) end
        game.write("main", address, bytes)
    end
    local function first_battle(): boolean
        return buffer.readu32(game.read("main", BATTLE_TICK, 4), 0) == 0
    end

    game.hook(offsets.start_screen_logo_entry, function() pc(offsets.start_screen_title_transition) end)
    game.hook(offsets.start_screen_sram_unmask_ret, function() pc(offsets.title_menu_continue) end)
    game.hook(offsets.game_load_ret, function()
        write(RNG1, 4, rng1)
        write(RNG2, 4, rng2)
    end)
    game.hook(offsets.title_menu_load_ret, function() pc(offsets.comm_menu_open) end)
    game.hook(offsets.comm_menu_top_confirm_gate, function()
        if first_battle() then
            write(SUBMENU + 0xd, 1, 0)
            pc(game.read_register("thumb_pc") + 8)
        end
    end)
    game.hook(offsets.comm_menu_bring_up_entry, function()
        if first_battle() then
            write(SUBMENU + 0x12, 1, battle_type)
            write(SUBMENU + 0x13, 1, 0)
            -- Enter the game's own session initialization. Its hello/settings/
            -- ready exchanges still run over the real emulated link cable.
            pc(offsets.comm_menu_start_battle)
        end
    end)
    game.hook(offsets.comm_menu_ready_entry, function() write(SUBMENU + 0x26, 2, 4) end)
    -- Random battles go live before rank selection and folder generation.
    -- Both players perform that setup themselves through the game's UI.
    game.hook(offsets.comm_menu_rank_entry, game.ready)
    game.hook(offsets.comm_menu_rank_cancel_entry, function()
        if first_battle() then game.match_aborted() end
    end)
    lifecycle(context, game, offsets)
end
