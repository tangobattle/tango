use crate::hooks::Trap;

use super::rng::{generate_rng_state, pick_rng_state};
use super::INIT_RX;

pub(super) fn traps(hooks: &super::Hooks, shadow_state: crate::shadow::State) -> Vec<Trap> {
    let make_send_and_receive_call_hook = || {
        let shadow_state = shadow_state.clone();
        let munger = hooks.munger();

        Box::new(move |mut core: mgba::core::CoreMutRef| {
            let pc = core.as_ref().gba().cpu().thumb_pc();
            core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);

            let mut round_state = shadow_state.lock_round_state();
            let round = match round_state.round.as_mut() {
                Some(round) => round,
                None => {
                    core.gba_mut().cpu_mut().set_gpr(0, 0);
                    return;
                }
            };
            core.gba_mut().cpu_mut().set_gpr(0, 3);

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
            round.set_remote_packet(munger.tx_packet(core).to_vec());
            round.set_input_injected();
        })
    };

    vec![
        (hooks.offsets.rom.comm_menu_init_ret, {
            let munger = hooks.munger();
            let shadow_state = shadow_state.clone();
            Box::new(move |core| {
                let mut rng = shadow_state.lock_rng();
                let non_shared_rng_state = pick_rng_state(&mut *rng, !shadow_state.is_offerer());
                munger.set_rng_state(core, non_shared_rng_state);
                munger.start_battle_from_comm_menu(core);
            })
        }),
        (hooks.offsets.rom.comm_menu_settings_entry, {
            let munger = hooks.munger();
            let shadow_state = shadow_state.clone();
            Box::new(move |mut core| {
                let mut rng = shadow_state.lock_rng();
                let shared_rng_seed = generate_rng_state(&mut *rng);
                munger.set_rng_state(core, shared_rng_seed);
                munger.select_battle_init_substate(core, 0x2c);
                let pc = core.as_ref().gba().cpu().thumb_pc();
                core.gba_mut().cpu_mut().set_thumb_pc(pc + 0x70);
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
        (hooks.offsets.rom.link_is_p2_ret, {
            let shadow_state = shadow_state.clone();
            Box::new(move |mut core: mgba::core::CoreMutRef| {
                let mut round_state = shadow_state.lock_round_state();
                let Some(round) = round_state.round.as_mut() else {
                    return;
                };
                core.gba_mut().cpu_mut().set_gpr(0, round.remote_player_index() as i32);
            })
        }),
        (hooks.offsets.rom.main_read_joyflags, {
            let munger = hooks.munger();
            let shadow_state = shadow_state.clone();
            Box::new(move |mut core: mgba::core::CoreMutRef| {
                let mut round_state = shadow_state.lock_round_state();
                let Some(round) = round_state.round.as_mut() else {
                    return;
                };

                if !munger.is_linking(core) && !round.has_first_committed_state() {
                    let mut rng = shadow_state.lock_rng();
                    let shared_rng_state = generate_rng_state(&mut *rng);
                    munger.set_rng_state(core, shared_rng_state);
                    return;
                }

                if !round.has_first_committed_state() {
                    round.set_first_committed_state(core.save_state().expect("save state"), &munger.tx_packet(core));
                    // Halt run_loop at the snapshot so it can't over-run the committed tick.
                    core.end_run_loop();
                    log::info!("shadow rng state: {:08x}", munger.rng_state(core));
                    log::info!("shadow state committed on {}", round.current_tick());
                    return;
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
        (
            hooks.offsets.rom.handle_input_custom_send_and_receive_call,
            make_send_and_receive_call_hook(),
        ),
        (
            hooks.offsets.rom.handle_input_in_turn_send_and_receive_call,
            make_send_and_receive_call_hook(),
        ),
        {
            let munger = hooks.munger();
            (
                hooks.offsets.rom.comm_menu_send_and_receive_call,
                Box::new(move |mut core| {
                    let pc = core.as_ref().gba().cpu().thumb_pc();
                    core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
                    core.gba_mut().cpu_mut().set_gpr(0, 3);
                    munger.set_rx_packet(core, 0, &INIT_RX);
                    munger.set_rx_packet(core, 1, &INIT_RX);
                }),
            )
        },
        (
            hooks.offsets.rom.init_sio_call,
            Box::new(|mut core| {
                let pc = core.as_ref().gba().cpu().thumb_pc();
                core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
            }),
        ),
        (hooks.offsets.rom.round_call_jump_table_ret, {
            let shadow_state = shadow_state.clone();
            Box::new(move |_core| {
                let mut round_state = shadow_state.lock_round_state();
                let Some(round) = round_state.round.as_mut() else {
                    return;
                };
                if !round.has_first_committed_state() {
                    return;
                }
                round.increment_current_tick();
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
