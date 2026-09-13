--!strict
local install_hooks = require("./hooks")
local rom_offsets = require("./offsets")
local rng = require("@bn-common/gamemode/rng")

return function()
    local seed = table.create(16, 0)
    assert(rng(seed, 1, 1) == 0x9e3779b9 and rng(seed, 1, 2) == 0x3c6ef372)
    assert(rng(seed, 2, 1) == 0xdaa66d2b and rng(seed, 2, 2) == 0x78dde6e4)
    seed[1], seed[2], seed[3], seed[4] = 0xb9, 0x79, 0x37, 0x9e
    assert(rng(seed, 1, 1) == 1)

    for _, offsets in rom_offsets do
        for player = 1, 2 do
            for battle_type = 0, 2 do
                for _, disable_bgm in {false, true} do
                    local tick, pc = 0, 0x08001234
                    local callbacks: {[number]: () -> ()} = {}
                    local writes: {[number]: buffer} = {}
                    local events: {string} = {}
                    local game: Game = {
                        hook = function(address: number, callback: () -> ())
                            assert(callbacks[address] == nil)
                            callbacks[address] = callback
                        end,
                        read = function(space: string, address: number, length: number): buffer
                            assert(space == "main" and address == 0x020348e0 and length == 4)
                            local bytes = buffer.create(4)
                            buffer.writeu32(bytes, 0, tick)
                            return bytes
                        end,
                        write = function(space: string, address: number, bytes: buffer)
                            assert(space == "main")
                            writes[address] = bytes
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
                        table.clear(writes)
                        table.clear(events)
                        callbacks[address]()
                    end
                    invoke(offsets.start_screen_logo_entry)
                    assert(pc == offsets.start_screen_title_transition)
                    invoke(offsets.start_screen_sram_unmask_ret)
                    assert(pc == offsets.title_menu_continue)
                    invoke(offsets.game_load_ret)
                    assert(buffer.readu32(writes[0x02001120], 0) == rng(seed, player, 1))
                    assert(buffer.readu32(writes[0x020013f0], 0) == rng(seed, player, 2))
                    invoke(offsets.title_menu_load_ret)
                    assert(pc == offsets.comm_menu_open)
                    pc = 0x08001234
                    invoke(offsets.comm_menu_top_confirm_gate)
                    assert(pc == 0x0800123c and buffer.readu8(writes[0x02009a3d], 0) == 0)
                    invoke(offsets.comm_menu_bring_up_entry)
                    assert(pc == offsets.comm_menu_start_battle)
                    assert(buffer.readu8(writes[0x02009a42], 0) == battle_type)
                    assert(buffer.readu8(writes[0x02009a43], 0) == 0)
                    invoke(offsets.comm_menu_ready_entry)
                    assert(buffer.readu16(writes[0x02009a56], 0) == 4)
                    invoke(offsets.comm_menu_rank_cancel_entry)
                    assert(events[1] == "match_aborted")
                    tick, pc = 1, 0x08001234
                    for _, address in {offsets.comm_menu_top_confirm_gate, offsets.comm_menu_bring_up_entry, offsets.comm_menu_rank_cancel_entry} do
                        invoke(address)
                        assert(next(writes) == nil and #events == 0 and pc == 0x08001234)
                    end
                    invoke(offsets.battle_start_play_music_call)
                    assert(pc == (if disable_bgm then 0x08001238 else 0x08001234))
                    invoke(offsets.comm_menu_rank_entry)
                    assert(#events == 1 and events[1] == "ready")
                    invoke(offsets.round_start_ret)
                    assert(events[1] == "ready" and #events == (if player == 1 then 2 else 1))
                    if player == 1 then assert(events[2] == "round_started") end
                    for _, test in {
                        {address = offsets.round_end_set_win, outcome = "outcome:1"},
                        {address = offsets.round_end_set_loss, outcome = "outcome:2"},
                        {address = offsets.round_end_damage_judge_set_win, outcome = "outcome:1"},
                        {address = offsets.round_end_damage_judge_set_loss, outcome = "outcome:2"},
                        {address = offsets.round_end_damage_judge_set_draw, outcome = "outcome:nil"},
                    } do
                        invoke(test.address)
                        assert(#events == (if player == 1 then 1 else 0))
                        if player == 1 then assert(events[1] == test.outcome) end
                    end
                    invoke(offsets.comm_menu_end_battle_entry)
                    assert(events[1] == "match_ended")
                end
            end
        end
    end
end
