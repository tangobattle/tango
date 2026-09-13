--!strict
-- The two cores run the game's own battle loop. Only the first core reports
-- round verdicts; either core may reach the final menu handoff first.
return function(context: GameModeContext, game: Game, offsets: {[string]: number})
    game.hook(offsets.battle_start_play_music_call, function()
        if context.disable_bgm then game.write_register("thumb_pc", game.read_register("thumb_pc") + 4) end
    end)
    game.hook(offsets.round_start_ret, function()
        game.ready()
        if context.player == 1 then game.round_started() end
    end)
    local function outcome(winner: number?): () -> ()
        return function()
            if context.player == 1 then game.round_outcome(winner) end
        end
    end
    game.hook(offsets.round_end_set_win, outcome(1))
    game.hook(offsets.round_end_set_loss, outcome(2))
    game.hook(offsets.round_end_damage_judge_set_win, outcome(1))
    game.hook(offsets.round_end_damage_judge_set_loss, outcome(2))
    game.hook(offsets.round_end_damage_judge_set_draw, outcome(nil))
    game.hook(offsets.comm_menu_end_battle_entry, game.match_ended)
end
