use crate::hooks::Trap;
use crate::stepper::BattleOutcome;

use super::rng::{generate_rng2_state, pick_rng_states};

pub(super) fn traps(hooks: &super::Hooks, stepper_state: crate::stepper::State) -> Vec<Trap> {
    vec![
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
            Box::new(move |core| {
                let stepper_state = stepper_state.lock_inner();
                let Some(rng) = stepper_state.replay_rng().cloned() else {
                    return;
                };
                let mut rng = rng.lock();
                let seed = generate_rng2_state(&mut *rng);
                munger.set_rng1_state(core, seed);
                munger.set_rng2_state(core, seed);
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
        (hooks.offsets.rom.battle_is_p2_tst, {
            let stepper_state = stepper_state.clone();
            Box::new(move |mut core: mgba::core::CoreMutRef| {
                let state = stepper_state.lock_inner();
                core.gba_mut().cpu_mut().set_gpr(0, state.local_player_index() as i32);
            })
        }),
        (hooks.offsets.rom.link_is_p2_ret, {
            let stepper_state = stepper_state.clone();
            Box::new(move |mut core: mgba::core::CoreMutRef| {
                let state = stepper_state.lock_inner();
                core.gba_mut().cpu_mut().set_gpr(0, state.local_player_index() as i32);
            })
        }),
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
                let current_tick = state.current_tick();

                if current_tick == state.commit_tick() && !state.has_committed_this_round() && state.round_active() {
                    if let Some(rng) = state.replay_rng().cloned() {
                        let mut rng = rng.lock();
                        let (rng1_state, rng2_state) = pick_rng_states(&mut *rng, state.replay_is_offerer());
                        munger.set_rng1_state(core, rng1_state);
                        munger.set_rng2_state(core, rng2_state);
                    }
                    state.set_local_packet(munger.tx_packet(core).to_vec());
                    state.set_committed_state(core.save_state().expect("save committed state"));
                }

                let Some(ip) = state.peek_input_pair().cloned() else {
                    return;
                };

                core.gba_mut().cpu_mut().set_gpr(4, (ip.local.joyflags | 0xfc00) as i32);

                if current_tick == state.dirty_tick() {
                    state.set_local_packet(munger.tx_packet(core).to_vec());
                    state.set_dirty_state(core.save_state().expect("save dirty state"));
                }
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

                let Some(ip) = state.pop_input_pair() else {
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
                munger.set_rx_packet(
                    core,
                    state.remote_player_index() as u32,
                    &state
                        .apply_shadow_input(crate::input::Pair {
                            local: ip.local.with_packet(local_packet),
                            remote: ip.remote,
                        })
                        .expect("apply shadow input")
                        .try_into()
                        .unwrap(),
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
                state.set_local_packet(munger.tx_packet(core).to_vec());
            })
        }),
        (hooks.offsets.rom.round_call_jump_table_ret, {
            let stepper_state = stepper_state.clone();
            Box::new(move |_core| {
                let mut state = stepper_state.lock_inner();
                state.increment_current_tick();
            })
        }),
        (hooks.offsets.rom.round_end_set_win, {
            let stepper_state = stepper_state.clone();
            Box::new(move |_core| {
                let mut state = stepper_state.lock_inner();
                state.set_round_result(BattleOutcome::Win);
            })
        }),
        (hooks.offsets.rom.round_end_set_loss, {
            let stepper_state = stepper_state.clone();
            Box::new(move |_core| {
                let mut state = stepper_state.lock_inner();
                state.set_round_result(BattleOutcome::Loss);
            })
        }),
        (hooks.offsets.rom.round_end_damage_judge_set_win, {
            let stepper_state = stepper_state.clone();
            Box::new(move |_core| {
                let mut state = stepper_state.lock_inner();
                state.set_round_result(BattleOutcome::Win);
            })
        }),
        (hooks.offsets.rom.round_end_damage_judge_set_loss, {
            let stepper_state = stepper_state.clone();
            Box::new(move |_core| {
                let mut state = stepper_state.lock_inner();
                state.set_round_result(BattleOutcome::Loss);
            })
        }),
        (hooks.offsets.rom.round_end_damage_judge_set_draw, {
            let stepper_state = stepper_state.clone();
            Box::new(move |_core| {
                let mut state = stepper_state.lock_inner();
                state.set_round_result(BattleOutcome::Draw);
            })
        }),
    ]
}
