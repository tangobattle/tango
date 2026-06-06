use crate::hooks::Trap;

use super::rng::{generate_rng2_state, pick_rng_states};

pub(super) fn traps(hooks: &super::Hooks, shadow_state: crate::shadow::State) -> Vec<Trap> {
    vec![
        (hooks.offsets.rom.comm_menu_init_ret, {
            let munger = hooks.munger();
            let shadow_state = shadow_state.clone();
            Box::new(move |core| {
                munger.start_battle_from_comm_menu(core, shadow_state.match_type().0);
            })
        }),
        (hooks.offsets.rom.round_start_ret, {
            let shadow_state = shadow_state.clone();
            Box::new(move |_core| {
                shadow_state.start_round();
            })
        }),
        (hooks.offsets.rom.round_end_entry, {
            let shadow_state = shadow_state.clone();
            Box::new(move |mut core| {
                shadow_state.end_round();
                shadow_state.set_applied_state(core.save_state().expect("save state"), 0);
                // Halt run_loop at the snapshot so it can't run past round end.
                core.end_run_loop();
            })
        }),
        (hooks.offsets.rom.battle_is_p2_tst, {
            let shadow_state = shadow_state.clone();
            Box::new(move |mut core| {
                let mut round_state = shadow_state.lock_round_state();
                let Some(round) = round_state.round.as_mut() else {
                    return;
                };
                core.gba_mut().cpu_mut().set_gpr(0, round.remote_player_index() as i32);
            })
        }),
        (hooks.offsets.rom.link_is_p2_ret, {
            let shadow_state = shadow_state.clone();
            Box::new(move |mut core| {
                let mut round_state = shadow_state.lock_round_state();
                let Some(round) = round_state.round.as_mut() else {
                    return;
                };
                core.gba_mut().cpu_mut().set_gpr(0, round.remote_player_index() as i32);
            })
        }),
        (
            hooks.offsets.rom.handle_sio_entry,
            Box::new(move |core| {
                log::error!(
                    "unhandled call to handleSIO at 0x{:0x}: uh oh!",
                    core.as_ref().gba().cpu().gpr(14) - 2
                );
            }),
        ),
        (hooks.offsets.rom.comm_menu_settings_entry, {
            let munger = hooks.munger();
            let shadow_state = shadow_state.clone();
            Box::new(move |mut core| {
                let mut rng = shadow_state.lock_rng();
                let seed = generate_rng2_state(&mut *rng);
                munger.set_rng1_state(core, seed);
                munger.set_rng2_state(core, seed);
                munger.select_init_battle_substate(core);
                let pc = core.as_ref().gba().cpu().thumb_pc();
                core.gba_mut().cpu_mut().set_thumb_pc(pc + 0x12);
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
        (hooks.offsets.rom.main_read_joyflags, {
            let munger = hooks.munger();
            let shadow_state = shadow_state.clone();
            Box::new(move |mut core| {
                let mut round_state = shadow_state.lock_round_state();
                let Some(round) = round_state.round.as_mut() else {
                    return;
                };

                if !round.has_first_committed_state() {
                    let mut rng = shadow_state.lock_rng();

                    // rng1 is the local rng, it should not be synced.
                    // However, we should make sure it's reproducible from the shared RNG state so we generate it like this.
                    // rng2 is the shared rng, it must be synced.
                    let (rng1_state, rng2_state) = pick_rng_states(&mut *rng, !shadow_state.is_offerer());
                    munger.set_rng1_state(core, rng1_state);
                    munger.set_rng2_state(core, rng2_state);

                    // HACK: The battle jump table goes directly from deinit to init, so we actually end up initializing on tick 1 after round 1. We just override it here.
                    munger.set_current_tick(core, 0);

                    round.set_first_committed_state(core.save_state().expect("save state"), &munger.tx_packet(core));
                    // Halt run_loop at the snapshot so it can't over-run the committed tick.
                    core.end_run_loop();
                    log::info!(
                        "shadow rng1 state: {:08x}, rng2 state: {:08x}",
                        munger.rng1_state(core),
                        munger.rng2_state(core),
                    );
                    log::info!("shadow state committed on {}", round.current_tick());
                    return;
                }

                let game_current_tick = munger.current_tick(core);
                if game_current_tick != round.current_tick() {
                    shadow_state.set_anyhow_error(anyhow::anyhow!(
                        "read joyflags: round tick = {} but game tick = {}",
                        round.current_tick(),
                        game_current_tick
                    ));
                }

                if let Some(pending) = round.peek_shadow_input() {
                    let (_local, remote) = &pending.pair;
                    core.gba_mut()
                        .cpu_mut()
                        .set_gpr(4, (remote.joyflags | 0xfc00) as i32);
                }

                if round.take_input_injected() {
                    shadow_state.set_applied_state(core.save_state().expect("save state"), round.current_tick());
                    // Halt run_loop at the snapshot so it can't spill past the applied tick.
                    core.end_run_loop();
                }
            })
        }),
        (hooks.offsets.rom.copy_input_data_entry, {
            let munger = hooks.munger();
            let shadow_state = shadow_state.clone();
            Box::new(move |core| {
                let mut round_state = shadow_state.lock_round_state();
                let Some(round) = round_state.round.as_mut() else {
                    return;
                };
                let game_current_tick = munger.current_tick(core);
                if game_current_tick != round.current_tick() {
                    shadow_state.set_anyhow_error(anyhow::anyhow!(
                        "copy input data: round tick = {} but game tick = {}",
                        round.current_tick(),
                        game_current_tick
                    ));
                }

                let Some(pending) = round.take_shadow_input() else {
                    return;
                };
                let (local, _remote) = pending.pair;

                if let Err(e) = round.check_remote_packet_at_current_tick() {
                    shadow_state.set_anyhow_error(e);
                    return;
                }

                let remote_packet = round.peek_remote_packet().unwrap();

                munger.set_rx_packet(
                    core,
                    round.local_player_index() as u32,
                    &local.packet.try_into().unwrap(),
                );
                munger.set_rx_packet(
                    core,
                    round.remote_player_index() as u32,
                    &remote_packet.clone().try_into().unwrap(),
                );
            })
        }),
        (hooks.offsets.rom.copy_input_data_ret, {
            let munger = hooks.munger();
            let shadow_state = shadow_state.clone();
            Box::new(move |core| {
                let mut round_state = shadow_state.lock_round_state();
                let Some(round) = round_state.round.as_mut() else {
                    return;
                };
                let game_current_tick = munger.current_tick(core);
                if game_current_tick != round.current_tick() {
                    shadow_state.set_anyhow_error(anyhow::anyhow!(
                        "copy input data: round tick = {} but game tick = {}",
                        round.current_tick(),
                        game_current_tick
                    ));
                }

                round.set_remote_packet(munger.tx_packet(core).to_vec());
                round.set_input_injected();
            })
        }),
        (hooks.offsets.rom.round_post_increment_tick, {
            let munger = hooks.munger();
            let shadow_state = shadow_state.clone();
            Box::new(move |core| {
                let mut round_state = shadow_state.lock_round_state();
                let Some(round) = round_state.round.as_mut() else {
                    return;
                };
                if !round.has_first_committed_state() {
                    return;
                }
                round.increment_current_tick();

                let game_current_tick = munger.current_tick(core);
                if game_current_tick != round.current_tick() {
                    shadow_state.set_anyhow_error(anyhow::anyhow!(
                        "post increment tick: round tick = {} but game tick = {}",
                        round.current_tick(),
                        game_current_tick
                    ));
                }
            })
        }),
        (hooks.offsets.rom.round_end_set_win, {
            let shadow_state = shadow_state.clone();
            Box::new(move |_core| {
                shadow_state.lock_round_state().set_result_is_in();
            })
        }),
        (hooks.offsets.rom.round_end_set_loss, {
            let shadow_state = shadow_state.clone();
            Box::new(move |_core| {
                shadow_state.lock_round_state().set_result_is_in();
            })
        }),
        (hooks.offsets.rom.round_end_damage_judge_set_win, {
            let shadow_state = shadow_state.clone();
            Box::new(move |_core| {
                shadow_state.lock_round_state().set_result_is_in();
            })
        }),
        (hooks.offsets.rom.round_end_damage_judge_set_loss, {
            let shadow_state = shadow_state.clone();
            Box::new(move |_core| {
                shadow_state.lock_round_state().set_result_is_in();
            })
        }),
        (hooks.offsets.rom.round_end_damage_judge_set_draw, {
            let shadow_state = shadow_state.clone();
            Box::new(move |_core| {
                shadow_state.lock_round_state().set_result_is_in();
            })
        }),
    ]
}
