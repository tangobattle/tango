use crate::hooks::{shadow_result_is_in_traps, shadow_round_trap, shadow_trap, Trap};

use super::rng::{generate_rng1_state, generate_rng2_state, random_battle_settings_and_background};

pub(super) fn traps(hooks: &super::Hooks, shadow_state: crate::shadow::State) -> Vec<Trap> {
    let mut traps: Vec<Trap> = vec![
        {
            let munger = hooks.munger();
            shadow_trap(hooks.offsets.rom.comm_menu_init_ret, &shadow_state, move |shadow_state, core| {
                munger.start_battle_from_comm_menu(core, shadow_state.match_type().0);
            })
        },
        shadow_trap(hooks.offsets.rom.round_start_ret, &shadow_state, |shadow_state, _core| {
            shadow_state.start_round();
        }),
        shadow_trap(hooks.offsets.rom.round_end_entry, &shadow_state, |shadow_state, core| {
            shadow_state.end_round();
            shadow_state.set_applied_state(core.save_state().expect("save state"), 0);
        }),
        shadow_round_trap(hooks.offsets.rom.battle_is_p2_tst, &shadow_state, |_shadow_state, round, mut core| {
            core.gba_mut().cpu_mut().set_gpr(0, round.remote_player_index() as i32);
        }),
        shadow_round_trap(hooks.offsets.rom.link_is_p2_ret, &shadow_state, |_shadow_state, round, mut core| {
            core.gba_mut().cpu_mut().set_gpr(0, round.remote_player_index() as i32);
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
        {
            let munger = hooks.munger();
            shadow_trap(hooks.offsets.rom.comm_menu_init_battle_entry, &shadow_state, move |shadow_state, core| {
                let mut rng = shadow_state.lock_rng();
                munger.set_link_battle_settings_and_background(
                    core,
                    random_battle_settings_and_background(&mut *rng, shadow_state.match_type().0),
                );
            })
        },
        {
            let munger = hooks.munger();
            (
                hooks.offsets.rom.comm_menu_in_battle_call_comm_menu_handle_link_cable_input,
                Box::new(move |mut core| {
                    let pc = core.as_ref().gba().cpu().thumb_pc();
                    core.gba_mut().cpu_mut().set_thumb_pc(pc + 6);
                    munger.set_copy_data_input_state(core, 2);
                }),
            )
        },
        {
            let munger = hooks.munger();
            shadow_round_trap(hooks.offsets.rom.main_read_joyflags, &shadow_state, move |shadow_state, round, mut core| {
                if !round.has_first_committed_state() {
                    let mut rng = shadow_state.lock_rng();

                    // rng1 is the local rng, it should not be synced.
                    // However, we should make sure it's reproducible from the shared RNG state so we generate it like this.
                    let offerer_rng1_state = generate_rng1_state(&mut *rng);
                    let answerer_rng1_state = generate_rng1_state(&mut *rng);
                    munger.set_rng1_state(
                        core,
                        if shadow_state.is_offerer() {
                            answerer_rng1_state
                        } else {
                            offerer_rng1_state
                        },
                    );

                    // rng2 is the shared rng, it must be synced.
                    let rng2_state = generate_rng2_state(&mut *rng);
                    munger.set_rng2_state(core, rng2_state);

                    // HACK: The battle jump table goes directly from deinit to init, so we actually end up initializing on tick 1 after round 1. We just override it here.
                    munger.set_current_tick(core, 0);

                    round.set_first_committed_state(core.save_state().expect("save state"), &munger.tx_packet(core));
                    log::info!(
                        "shadow rng1 state: {:08x}, rng2 state: {:08x}",
                        munger.rng1_state(core),
                        munger.rng2_state(core)
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
                    core.gba_mut()
                        .cpu_mut()
                        .set_gpr(4, (pending.pair.remote.joyflags | 0xfc00) as i32);
                }

                if round.take_input_injected() {
                    shadow_state.set_applied_state(core.save_state().expect("save state"), round.current_tick());
                }
            })
        },
        {
            let munger = hooks.munger();
            shadow_round_trap(hooks.offsets.rom.copy_input_data_entry, &shadow_state, move |shadow_state, round, core| {
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
            })
        },
        {
            let munger = hooks.munger();
            shadow_round_trap(hooks.offsets.rom.copy_input_data_ret, &shadow_state, move |shadow_state, round, core| {
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
        },
        {
            let munger = hooks.munger();
            shadow_round_trap(hooks.offsets.rom.round_post_increment_tick, &shadow_state, move |shadow_state, round, core| {
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
        },
    ];
    traps.extend(shadow_result_is_in_traps(
        &shadow_state,
        &[
            hooks.offsets.rom.round_end_set_win,
            hooks.offsets.rom.round_end_set_loss,
            hooks.offsets.rom.round_end_damage_judge_set_win,
            hooks.offsets.rom.round_end_damage_judge_set_loss,
            hooks.offsets.rom.round_end_damage_judge_set_draw,
        ],
    ));
    traps
}
