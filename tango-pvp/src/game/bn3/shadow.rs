use byteorder::ByteOrder;

use crate::hooks::Trap;

use super::rng::{bn3_match_type, generate_rng2_state, pick_rng1_state};
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

            let current_tick = round.current_tick();

            let Some(pending) = round.take_shadow_input() else {
                let mut rx = [0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
                byteorder::LittleEndian::write_u32(&mut rx[4..8], current_tick.saturating_sub(2));
                munger.set_rx_packet(core, 0, &rx);
                munger.set_rx_packet(core, 1, &rx);
                return;
            };

            // HACK: This is required if the emulator advances beyond read joyflags and runs this function again, but is missing input data.
            // We permit this for one tick only, but really we should just not be able to get into this situation in the first place.
            if pending.expected_tick + 1 == round.current_tick() {
                return;
            }

            if let Err(e) = round.check_remote_packet_at_current_tick() {
                shadow_state.set_anyhow_error(e);
                return;
            }

            let remote_packet = round.peek_remote_packet().unwrap();

            munger.set_rx_packet(
                core,
                round.local_player_index() as u32,
                &pending.pair.local.packet.try_into().unwrap(),
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

                // rng1 is the local rng, it should not be synced.
                // However, we should make sure it's reproducible from the shared RNG state so we generate it like this.
                let rng1_state = pick_rng1_state(&mut *rng, !shadow_state.is_offerer());
                munger.set_rng1_state(core, rng1_state);
                munger.start_battle_from_comm_menu(core, bn3_match_type(&mut *rng, shadow_state.match_type()));
            })
        }),
        (hooks.offsets.rom.comm_menu_settings_entry, {
            let munger = hooks.munger();
            let shadow_state = shadow_state.clone();
            Box::new(move |mut core| {
                let mut rng = shadow_state.lock_rng();
                let r2_seed = generate_rng2_state(&mut *rng);
                munger.set_rng2_state(core, r2_seed);
                munger.select_battle_init_substate(core, 0x30);
                let pc = core.as_ref().gba().cpu().thumb_pc();
                core.gba_mut().cpu_mut().set_thumb_pc(pc + 0x50);
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
            Box::new(move |core| {
                shadow_state.end_round();
                shadow_state.set_applied_state(core.save_state().expect("save state"), 0);
            })
        }),
        (hooks.offsets.rom.battle_is_p2_ret, {
            let shadow_state = shadow_state.clone();
            Box::new(move |mut core| {
                let mut round_state = shadow_state.lock_round_state();
                let Some(round) = round_state.round.as_mut() else { return };
                core.gba_mut().cpu_mut().set_gpr(0, round.remote_player_index() as i32);
            })
        }),
        (hooks.offsets.rom.link_is_p2_ret, {
            let shadow_state = shadow_state.clone();
            Box::new(move |mut core| {
                let mut round_state = shadow_state.lock_round_state();
                let Some(round) = round_state.round.as_mut() else { return };
                core.gba_mut().cpu_mut().set_gpr(0, round.remote_player_index() as i32);
            })
        }),
        (hooks.offsets.rom.main_read_joyflags, {
            let munger = hooks.munger();
            let shadow_state = shadow_state.clone();
            Box::new(move |mut core| {
                let mut round_state = shadow_state.lock_round_state();
                let Some(round) = round_state.round.as_mut() else { return };

                if !munger.is_linking(core) && !round.has_first_committed_state() {
                    return;
                }

                if !round.has_first_committed_state() {
                    let mut rng = shadow_state.lock_rng();

                    // rng2 is the shared rng, it must be synced.
                    let rng2_state = generate_rng2_state(&mut *rng);
                    munger.set_rng2_state(core, rng2_state);

                    round.set_first_committed_state(core.save_state().expect("save state"), &munger.tx_packet(core));
                    log::info!(
                        "shadow rng1 state: {:08x}, rng2 state: {:08x}",
                        munger.rng1_state(core),
                        munger.rng2_state(core)
                    );
                    log::info!("shadow state committed on {}", round.current_tick());
                    return;
                }

                if let Some(pending) = round.peek_shadow_input() {
                    core.gba_mut()
                        .cpu_mut()
                        .set_gpr(4, (pending.pair.remote.joyflags | 0xfc00) as i32);
                }

                if round.take_input_injected() {
                    shadow_state.set_applied_state(core.save_state().expect("save state"), round.current_tick());
                }
            })
        }),
        (
            hooks.offsets.rom.handle_input_init_send_and_receive_call,
            make_send_and_receive_call_hook(),
        ),
        (
            hooks.offsets.rom.handle_input_update_send_and_receive_call,
            make_send_and_receive_call_hook(),
        ),
        (
            hooks.offsets.rom.handle_input_deinit_send_and_receive_call,
            make_send_and_receive_call_hook(),
        ),
        (
            hooks.offsets.rom.process_battle_input_ret,
            Box::new(move |mut core| {
                core.gba_mut().cpu_mut().set_gpr(0, 0);
            }),
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
            Box::new(move |mut core| {
                let mut round_state = shadow_state.lock_round_state();
                let Some(round) = round_state.round.as_mut() else {
                    return;
                };
                if !round.has_first_committed_state() {
                    return;
                }
                round.increment_current_tick();

                if round_state.result_is_in {
                    // We have no real inputs left but the round has ended. Just fudge them until we get to the next round.
                    core.gba_mut().cpu_mut().set_gpr(0, 7);
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
