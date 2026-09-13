--!strict
local rng = require("@bn-common/gamemode/rng")
local lifecycle = require("@bn-common/gamemode/lifecycle")
local memory = require("./memory")

return function(context: GameModeContext, game: Game, offsets: {[string]: number}, battle_type: number)
    local rng1 = rng(context.seed, context.player, 1)
    local rng2 = rng(context.seed, context.player, 2)
    local function pc(value: number) game.write_register("thumb_pc", value) end
    local function write(address: number, length: number, value: number)
        local bytes = buffer.create(length)
        if length == 1 then buffer.writeu8(bytes, 0, value)
        elseif length == 2 then buffer.writeu16(bytes, 0, value)
        else buffer.writeu32(bytes, 0, value) end
        game.write("main", address, bytes)
    end
    local function first_battle(): boolean
        return buffer.readu32(game.read("main", memory.battle_state + 0x60, 4), 0) == 0
    end
    local function menu_state(): number
        return buffer.readu8(game.read("main", memory.submenu_control + 2, 1), 0)
    end
    local function gate_present()
        write(memory.gate_flag, 1, 1)
        write(memory.gate_flag + 2, 2, 0xf0)
    end
    game.hook(offsets.start_screen_logo_entry, function() pc(offsets.start_screen_title_transition) end)
    game.hook(offsets.title_init_ret, function() pc(offsets.title_confirm_body) end)
    game.hook(offsets.game_load_ret, function()
        write(memory.rng1_state, 4, rng1)
        write(memory.rng2_state, 4, rng2)
    end)
    game.hook(offsets.field_menu_gate, function()
        if first_battle() then pc(offsets.field_menu_open_branch) end
    end)
    game.hook(offsets.start_menu_select_site, function()
        if not first_battle() then return end
        gate_present()
        write(memory.menu_control + 4, 1, 6)
        pc(offsets.start_menu_confirm_branch)
    end)
    game.hook(offsets.comm_menu_prompt_ret, function()
        if first_battle() and menu_state() == 4 then pc(offsets.comm_menu_prompt_press_branch) end
    end)
    game.hook(offsets.comm_menu_nav_ret, function()
        if not first_battle() or menu_state() ~= 8 then return end
        -- One confirm at the rules level: the ROM commits the selected team
        -- mode, initializes the session and negotiates over the real cable.
        write(memory.submenu_control + 0x14, 1, 2)
        write(memory.submenu_control + 0x2a, 2, 1)
        write(memory.submenu_control + 0x34, 2, battle_type)
        gate_present()
        write(memory.submenu_control + 0x3e, 2, 0)
        write(memory.submenu_control + 0x1c, 1, 1)
        pc(offsets.comm_menu_nav_confirm_branch)
    end)
    -- Hardware expects two Battle Chip Gates between the consoles. Skip only
    -- that device census; the surrounding settings exchange still runs.
    game.hook(offsets.team_census_site, function() pc(offsets.team_census_passed) end)
    lifecycle(context, game, offsets)
end
