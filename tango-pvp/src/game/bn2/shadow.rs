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

            let mut state = shadow_state.lock();
            let round = match state.round.as_mut() {
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

    let mut traps: Vec<Trap> = vec![
        (hooks.offsets.rom.comm_menu_init_ret, {
            let munger = hooks.munger();
            let shadow_state = shadow_state.clone();
            Box::new(move |core| {
                let mut state = shadow_state.lock();
                let non_shared_rng_state = pick_rng_state(&mut state.rng, !shadow_state.is_offerer());
                munger.set_rng_state(core, non_shared_rng_state);
                munger.start_battle_from_comm_menu(core);
            })
        }),
        (hooks.offsets.rom.comm_menu_settings_entry, {
            let munger = hooks.munger();
            let shadow_state = shadow_state.clone();
            Box::new(move |mut core| {
                let mut state = shadow_state.lock();
                let shared_rng_seed = generate_rng_state(&mut state.rng);
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
                // Round's over — halt run_loop here so it can't spill into the
                // inter-round transition.
                core.end_run_loop();
            })
        }),
        (hooks.offsets.rom.link_is_p2_ret, {
            let shadow_state = shadow_state.clone();
            Box::new(move |mut core: mgba::core::CoreMutRef| {
                let mut state = shadow_state.lock();
                let Some(round) = state.round.as_mut() else {
                    return;
                };
                core.gba_mut().cpu_mut().set_gpr(0, round.remote_player_index() as i32);
            })
        }),
        (hooks.offsets.rom.main_read_joyflags, {
            let munger = hooks.munger();
            let shadow_state = shadow_state.clone();
            Box::new(move |mut core: mgba::core::CoreMutRef| {
                let mut state = shadow_state.lock();
                let state = &mut *state;
                let Some(round) = state.round.as_mut() else {
                    return;
                };

                if !munger.is_linking(core) && !round.has_first_committed_state() {
                    let shared_rng_state = generate_rng_state(&mut state.rng);
                    munger.set_rng_state(core, shared_rng_state);
                    return;
                }

                if !round.has_first_committed_state() {
                    round.set_first_committed(&munger.tx_packet(core));
                    // Halt run_loop at the first committed tick so it can't over-run it.
                    core.end_run_loop();
                    log::info!("shadow rng state: {:08x}", munger.rng_state(core));
                    log::info!("shadow state committed on {}", round.current_tick());
                    return;
                }

                if let Some(pending) = round.peek_shadow_input() {
                    let (_local, remote) = &pending.pair;
                    core.gba_mut().cpu_mut().set_gpr(4, (remote.joyflags | !crate::input::JOYFLAGS_MASK) as i32);
                }

                if round.take_input_injected() {
                    // The input's been applied and the core has reached the next
                    // tick's read_joyflags; signal apply_input and stop here so
                    // run_loop parks the shadow exactly at that boundary.
                    state.input_applied = true;
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
                let mut state = shadow_state.lock();
                let Some(round) = state.round.as_mut() else {
                    return;
                };
                if !round.has_first_committed_state() {
                    return;
                }
                round.increment_current_tick();
            })
        }),
    ];

    // Every round-end verdict site just latches `result_is_in`; the shadow
    // doesn't track which side won.
    for offset in [
        hooks.offsets.rom.round_end_set_win,
        hooks.offsets.rom.round_end_set_loss,
        hooks.offsets.rom.round_end_damage_judge_set_win,
        hooks.offsets.rom.round_end_damage_judge_set_loss,
        hooks.offsets.rom.round_end_damage_judge_set_draw,
    ] {
        let shadow_state = shadow_state.clone();
        traps.push((
            offset,
            Box::new(move |_core: mgba::core::CoreMutRef| {
                shadow_state.lock().result_is_in = true;
            }),
        ));
    }

    traps
}
