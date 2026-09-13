--!strict
local install_hooks = require("./hooks")
local rom_offsets = require("./offsets")
local memory = require("./memory")
local rng = require("@bn-common/gamemode/rng")

return function()
    for _, offsets in rom_offsets do
        for player = 1, 2 do
            for battle_type = 0, 1 do
                for _, disable_bgm in {false, true} do
                    local ram = buffer.create(0x40000)
                    local seed = table.create(16, 0)
                    local pc = 0x08001234
                    local callbacks: {[number]: () -> ()} = {}
                    local writes, events = 0, {} :: {string}
                    local game: Game = {
                        hook = function(address: number, callback: () -> ())
                            assert(callbacks[address] == nil)
                            callbacks[address] = callback
                        end,
                        read = function(space: string, address: number, length: number): buffer
                            assert(space == "main")
                            local result = buffer.create(length)
                            buffer.copy(result, 0, ram, address - 0x02000000, length)
                            return result
                        end,
                        write = function(space: string, address: number, bytes: buffer)
                            assert(space == "main")
                            writes += 1
                            buffer.copy(ram, address - 0x02000000, bytes)
                        end,
                        read_register = function(name: string): number assert(name == "thumb_pc"); return pc end,
                        write_register = function(name: string, value: number) assert(name == "thumb_pc"); pc = value end,
                        ready = function() table.insert(events, "ready") end,
                        round_started = function() table.insert(events, "round_started") end,
                        round_outcome = function(winner: number?) table.insert(events, "outcome:" .. tostring(winner)) end,
                        match_ended = function() table.insert(events, "match_ended") end,
                        match_aborted = function() table.insert(events, "match_aborted") end,
                    }
                    install_hooks({player = player, seed = seed, disable_bgm = disable_bgm, options = {}}, game, offsets, battle_type)
                    local function invoke(address: number)
                        writes, pc = 0, 0x08001234
                        table.clear(events)
                        callbacks[address]()
                    end
                    invoke(offsets.start_screen_logo_entry)
                    assert(pc == offsets.start_screen_title_transition)
                    invoke(offsets.title_init_ret)
                    assert(pc == offsets.title_confirm_body)
                    invoke(offsets.game_load_ret)
                    assert(buffer.readu32(ram, memory.rng1_state - 0x02000000) == rng(seed, player, 1))
                    assert(buffer.readu32(ram, memory.rng2_state - 0x02000000) == rng(seed, player, 2))
                    invoke(offsets.field_menu_gate)
                    assert(pc == offsets.field_menu_open_branch)
                    invoke(offsets.start_menu_select_site)
                    assert(pc == offsets.start_menu_confirm_branch and writes == 3)
                    assert(buffer.readu8(ram, memory.gate_flag - 0x02000000) == 1)
                    assert(buffer.readu16(ram, memory.gate_flag + 2 - 0x02000000) == 0xf0)
                    assert(buffer.readu8(ram, memory.menu_control + 4 - 0x02000000) == 6)
                    invoke(offsets.comm_menu_prompt_ret)
                    assert(pc == 0x08001234)
                    buffer.writeu8(ram, memory.submenu_control + 2 - 0x02000000, 4)
                    invoke(offsets.comm_menu_prompt_ret)
                    assert(pc == offsets.comm_menu_prompt_press_branch)
                    invoke(offsets.comm_menu_nav_ret)
                    assert(pc == 0x08001234 and writes == 0)
                    buffer.writeu8(ram, memory.submenu_control + 2 - 0x02000000, 8)
                    invoke(offsets.comm_menu_nav_ret)
                    assert(pc == offsets.comm_menu_nav_confirm_branch and writes == 7)
                    assert(buffer.readu8(ram, memory.submenu_control + 0x14 - 0x02000000) == 2)
                    assert(buffer.readu16(ram, memory.submenu_control + 0x2a - 0x02000000) == 1)
                    assert(buffer.readu16(ram, memory.submenu_control + 0x34 - 0x02000000) == battle_type)
                    assert(buffer.readu16(ram, memory.submenu_control + 0x3e - 0x02000000) == 0)
                    assert(buffer.readu8(ram, memory.submenu_control + 0x1c - 0x02000000) == 1)
                    -- The post-battle menus must remain under the game's control.
                    buffer.writeu32(ram, memory.battle_state + 0x60 - 0x02000000, 1)
                    for _, address in {offsets.field_menu_gate, offsets.start_menu_select_site,
                        offsets.comm_menu_prompt_ret, offsets.comm_menu_nav_ret} do
                        invoke(address)
                        assert(writes == 0 and #events == 0 and pc == 0x08001234)
                    end
                    invoke(offsets.team_census_site)
                    assert(pc == offsets.team_census_passed)
                    invoke(offsets.battle_start_play_music_call)
                    assert(pc == (if disable_bgm then 0x08001238 else 0x08001234))
                    invoke(offsets.round_start_ret)
                    assert(events[1] == "ready" and #events == (if player == 1 then 2 else 1))
                    if player == 1 then assert(events[2] == "round_started") end
                    for _, case in {{offsets.round_end_set_win, 1}, {offsets.round_end_set_loss, 2},
                        {offsets.round_end_damage_judge_set_win, 1}, {offsets.round_end_damage_judge_set_loss, 2}} do
                        invoke(case[1])
                        assert(#events == (if player == 1 then 1 else 0))
                        if player == 1 then assert(events[1] == "outcome:" .. tostring(case[2])) end
                    end
                    invoke(offsets.round_end_damage_judge_set_draw)
                    assert(#events == (if player == 1 then 1 else 0))
                    if player == 1 then assert(events[1] == "outcome:nil") end
                    invoke(offsets.comm_menu_end_battle_entry)
                    assert(events[1] == "match_ended")
                end
            end
        end
    end
end
