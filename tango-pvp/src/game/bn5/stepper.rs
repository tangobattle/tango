use crate::hooks::Trap;
use crate::stepper::BattleOutcome;

use crate::game::shared::rng::{generate_rng2_state, pick_rng_states};

pub(super) fn traps(hooks: &super::Hooks, stepper_state: crate::stepper::State) -> Vec<Trap> {
    // Both player-index sites answer the same way: r0 = the replay's local
    // player index.
    let make_is_p2_hook = || {
        let stepper_state = stepper_state.clone();
        Box::new(move |mut core: mgba::core::CoreMutRef| {
            let state = stepper_state.lock_inner();
            core.gba_mut().cpu_mut().set_gpr(0, state.local_player_index() as i32);
        })
    };

    let mut traps: Vec<Trap> = vec![
        (hooks.offsets.rom.comm_menu_init_ret, {
            let munger = hooks.munger();
            let stepper_state = stepper_state.clone();
            Box::new(move |core| {
                let stepper_state = stepper_state.lock_inner();
                munger.start_battle_from_comm_menu(core, stepper_state.match_type().0);
            })
        }),
        (hooks.offsets.rom.comm_menu_settings_entry, {
            let munger = hooks.munger();
            let stepper_state = stepper_state.clone();
            Box::new(move |mut core| {
                let mut stepper_state = stepper_state.lock_inner();
                let Some(rng) = stepper_state.replay_rng_mut() else {
                    return;
                };
                let seed = generate_rng2_state(rng);
                munger.set_rng1_state(core, seed);
                munger.set_rng2_state(core, seed);
                munger.select_init_battle_substate(core);
                let pc = core.as_ref().gba().cpu().thumb_pc();
                core.gba_mut().cpu_mut().set_thumb_pc(pc + 0x12);
            })
        }),
        (hooks.offsets.rom.round_start_ret, {
            let stepper_state = stepper_state.clone();
            Box::new(move |_core| {
                stepper_state.lock_inner().advance_to_next_replay_round_if_pending();
            })
        }),
        (hooks.offsets.rom.battle_start_play_music_call, {
            let stepper_state = stepper_state.clone();
            Box::new(move |mut core: mgba::core::CoreMutRef| {
                let state = stepper_state.lock_inner();
                if !state.disable_bgm() {
                    return;
                }
                let pc = core.as_ref().gba().cpu().thumb_pc();
                core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
            })
        }),
        (hooks.offsets.rom.battle_is_p2_tst, make_is_p2_hook()),
        (hooks.offsets.rom.link_is_p2_ret, make_is_p2_hook()),
        {
            let munger = hooks.munger();
            (
                hooks.offsets.rom.in_battle_call_handle_link_cable_input,
                Box::new(move |mut core| {
                    let pc = core.as_ref().gba().cpu().thumb_pc();
                    core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
                    munger.set_copy_data_input_state(core, 2);
                }),
            )
        },
        (hooks.offsets.rom.round_set_ending, {
            let stepper_state = stepper_state.clone();
            Box::new(move |_core| {
                let mut state = stepper_state.lock_inner();
                state.set_round_ending();
            })
        }),
        (hooks.offsets.rom.round_end_entry, {
            let stepper_state = stepper_state.clone();
            Box::new(move |_core| {
                let mut state = stepper_state.lock_inner();
                state.set_round_ended();
            })
        }),
        (hooks.offsets.rom.main_read_joyflags, {
            let munger = hooks.munger();
            let stepper_state = stepper_state.clone();
            Box::new(move |mut core: mgba::core::CoreMutRef| {
                let mut state = stepper_state.lock_inner();
                // In replay mode, gate on round_active: this PC is hit in every
                // scene, not just battle. Without it the panic check below
                // would read battle_state outside a battle (matches the exe6
                // crash bn6 hit), and stale joyflags would land in r4 during
                // pre-battle code.
                if state.is_replaying() && !state.round_active() {
                    return;
                }
                let current_tick = state.current_tick();

                // Replay-mode-only first-commit hook: seed RNG, snap game tick
                // to 0, run the shadow-side first-commit advance. Never fires
                // in FF mode.
                if state.needs_replay_first_commit() {
                    let is_offerer = state.replay_is_offerer();
                    if let Some(rng) = state.replay_rng_mut() {
                        let (rng1_state, rng2_state) = pick_rng_states(rng, is_offerer);
                        munger.set_rng1_state(core, rng1_state);
                        munger.set_rng2_state(core, rng2_state);
                        // HACK: matches primary — BN5's jump table goes
                        // straight from deinit to init, leaving the game
                        // tick at 1 between rounds. Force it to 0 so
                        // the panic check below holds for round 2+.
                        munger.set_current_tick(core, 0);
                    }
                    state.set_local_packet(munger.tx_packet(core).to_vec());
                    state.on_first_commit();
                }

                assert_tick(current_tick, munger.current_tick(core));

                // FF state capture. At `capture_tick` the input window is
                // exhausted (all of `inputs` consumed), so there's no pair to
                // peek: snapshot poised at the start of the tick with r4 left
                // unset. The consumer injects the local joyflags — the live
                // primary via `inject_joyflags_on_primary_snapshot`, the next FF
                // by re-priming r4 at its first `main_read_joyflags`.
                // Never fires in replay mode.
                if state.at_capture_tick() {
                    state.capture();
                    // Halt run_loop at the capture: its leftover cycle budget must
                    // not spill into copy_input_data_entry and double-advance the
                    // shadow for the captured tick.
                    core.end_run_loop();
                    return;
                }

                let Some((local, _remote)) = state.peek_input_pair().cloned() else {
                    return;
                };

                core.gba_mut().cpu_mut().set_gpr(4, (local.joyflags | !crate::input::JOYFLAGS_MASK) as i32);
            })
        }),
        (hooks.offsets.rom.copy_input_data_entry, {
            let munger = hooks.munger();
            let stepper_state = stepper_state.clone();
            Box::new(move |core| {
                let mut state = stepper_state.lock_inner();
                if state.is_round_ending() {
                    return;
                }
                if state.is_replaying() && !state.has_committed_this_round() {
                    return;
                }

                let current_tick = state.current_tick();

                assert_tick(current_tick, munger.current_tick(core));

                let Some((local, remote)) = state.pop_input_pair() else {
                    return;
                };

                if let Err(e) = state.check_local_packet_at_current_tick() {
                    state.set_anyhow_error(e);
                    return;
                }

                let local_packet = state.peek_local_packet().unwrap().to_vec();

                munger.set_rx_packet(
                    core,
                    state.local_player_index() as u32,
                    &local_packet.clone().try_into().unwrap(),
                );
                // On failure the error is already on the stepper's error
                // channel; the drive loop aborts on it.
                let Some(remote_packet) = state.apply_shadow_input((local.with_packet(local_packet), remote)) else {
                    return;
                };
                munger.set_rx_packet(
                    core,
                    state.remote_player_index() as u32,
                    &remote_packet.try_into().unwrap(),
                );
            })
        }),
        (hooks.offsets.rom.copy_input_data_ret, {
            let munger = hooks.munger();
            let stepper_state = stepper_state.clone();
            Box::new(move |core| {
                let mut state = stepper_state.lock_inner();
                if state.is_round_ending() {
                    return;
                }
                if state.is_replaying() && !state.has_committed_this_round() {
                    return;
                }

                let current_tick = state.current_tick();

                assert_tick(current_tick, munger.current_tick(core));

                state.set_local_packet(munger.tx_packet(core).to_vec());
            })
        }),
        (hooks.offsets.rom.round_post_increment_tick, {
            let munger = hooks.munger();
            let stepper_state = stepper_state.clone();
            Box::new(move |core| {
                let mut state = stepper_state.lock_inner();
                if state.is_replaying() && !state.has_committed_this_round() {
                    return;
                }
                state.increment_current_tick();
                let current_tick = state.current_tick();

                let game_current_tick = munger.current_tick(core);
                if game_current_tick != current_tick {
                    state.set_anyhow_error(anyhow::anyhow!(
                        "post increment tick: round tick = {} but game tick = {}",
                        current_tick,
                        game_current_tick
                    ));
                }
            })
        }),
    ];

    // Every round-end verdict site just records the outcome.
    for (offset, outcome) in [
        (hooks.offsets.rom.round_end_set_win, BattleOutcome::Win),
        (hooks.offsets.rom.round_end_set_loss, BattleOutcome::Loss),
        (hooks.offsets.rom.round_end_damage_judge_set_win, BattleOutcome::Win),
        (hooks.offsets.rom.round_end_damage_judge_set_loss, BattleOutcome::Loss),
        (hooks.offsets.rom.round_end_damage_judge_set_draw, BattleOutcome::Draw),
    ] {
        let stepper_state = stepper_state.clone();
        traps.push((
            offset,
            Box::new(move |_core: mgba::core::CoreMutRef| {
                stepper_state.lock_inner().set_round_result(outcome);
            }),
        ));
    }

    traps
}

/// Panic if the game's battle tick has drifted from the replay round's. The
/// stepper drives the emulator deterministically, so any drift is a bug; the
/// panic is reported at the call site via `#[track_caller]`.
#[track_caller]
fn assert_tick(round_tick: u32, game_tick: u32) {
    if game_tick != round_tick {
        panic!("round tick = {round_tick} but game tick = {game_tick}");
    }
}
