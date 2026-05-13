use rand::Rng;

use crate::hooks::{shadow_result_is_in_traps, shadow_round_trap, shadow_trap, Trap};

use super::rng::generate_rng_state;
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

    let mut traps: Vec<Trap> = vec![
        (hooks.offsets.rom.comm_menu_init_ret, {
            let munger = hooks.munger();
            Box::new(move |core| {
                munger.start_battle_from_comm_menu(core);
            })
        }),
        {
            let munger = hooks.munger();
            shadow_trap(hooks.offsets.rom.round_start_ret, &shadow_state, move |shadow_state, core| {
                shadow_state.start_round();
                let mut rng = shadow_state.lock_rng();
                munger.set_battle_stage(core, rng.gen_range(0..0xc));
            })
        },
        shadow_trap(hooks.offsets.rom.round_end_entry, &shadow_state, |shadow_state, core| {
            shadow_state.end_round();
            shadow_state.set_applied_state(core.save_state().expect("save state"), 0);
        }),
        shadow_round_trap(hooks.offsets.rom.link_is_p2_ret, &shadow_state, |_shadow_state, round, mut core| {
            core.gba_mut().cpu_mut().set_gpr(0, round.remote_player_index() as i32);
        }),
        {
            let munger = hooks.munger();
            shadow_round_trap(hooks.offsets.rom.main_read_joyflags, &shadow_state, move |shadow_state, round, mut core| {
                if !round.has_first_committed_state() {
                    let mut rng = shadow_state.lock_rng();
                    let offerer_rng_state = generate_rng_state(&mut *rng);
                    let answerer_rng_state = generate_rng_state(&mut *rng);
                    munger.set_rng_state(
                        core,
                        if shadow_state.is_offerer() {
                            answerer_rng_state
                        } else {
                            offerer_rng_state
                        },
                    );

                    round.set_first_committed_state(core.save_state().expect("save state"), &munger.tx_packet(core));
                    log::info!("shadow rng state: {:08x}", munger.rng_state(core));
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
        },
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
        shadow_round_trap(hooks.offsets.rom.round_call_jump_table_ret, &shadow_state, |_shadow_state, round, _core| {
            if !round.has_first_committed_state() {
                return;
            }
            round.increment_current_tick();
        }),
    ];
    traps.extend(shadow_result_is_in_traps(
        &shadow_state,
        &[
            hooks.offsets.rom.round_end_set_win,
            hooks.offsets.rom.round_end_set_loss,
        ],
    ));
    traps
}
