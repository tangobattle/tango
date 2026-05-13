use byteorder::ByteOrder;

use crate::hooks::{stepper_round_outcome_traps, stepper_trap, Trap};
use crate::stepper::BattleOutcome;

use super::rng::{bn3_match_type, generate_rng1_state, generate_rng2_state, random_background};
use super::INIT_RX;

pub(super) fn traps(hooks: &super::Hooks, stepper_state: crate::stepper::State) -> Vec<Trap> {
    let make_send_and_receive_call_hook = || {
        let munger = hooks.munger();
        let stepper_state = stepper_state.clone();
        Box::new(move |mut core: mgba::core::CoreMutRef| {
            let mut stepper_state = stepper_state.lock_inner();

            let pc = core.as_ref().gba().cpu().thumb_pc();
            core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
            core.gba_mut().cpu_mut().set_gpr(0, 3);

            if stepper_state.replay_rng().is_some() && !stepper_state.has_committed_this_round() {
                return;
            }

            if stepper_state.is_round_ending() {
                return;
            }

            let Some(ip) = stepper_state.pop_input_pair() else {
                let mut rx = [0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
                let current_tick = stepper_state.current_tick();
                byteorder::LittleEndian::write_u32(&mut rx[0x04..0x08], current_tick.saturating_sub(2));
                munger.set_rx_packet(core, 0, &rx);
                munger.set_rx_packet(core, 1, &rx);
                return;
            };

            if let Err(e) = stepper_state.check_local_packet_at_current_tick() {
                stepper_state.set_anyhow_error(e);
                return;
            }

            let local_packet = stepper_state.peek_local_packet().unwrap().to_vec();

            munger.set_rx_packet(
                core,
                stepper_state.local_player_index() as u32,
                &local_packet.clone().try_into().unwrap(),
            );
            munger.set_rx_packet(
                core,
                stepper_state.remote_player_index() as u32,
                &stepper_state
                    .apply_shadow_input(crate::input::Pair {
                        local: ip.local.with_packet(local_packet),
                        remote: ip.remote,
                    })
                    .expect("apply shadow input")
                    .try_into()
                    .unwrap(),
            );
            stepper_state.set_local_packet(munger.tx_packet(core).to_vec());
        })
    };

    let mut traps: Vec<Trap> = vec![
        (hooks.offsets.rom.comm_menu_init_ret, {
            let munger = hooks.munger();
            let stepper_state = stepper_state.clone();
            Box::new(move |core| {
                let stepper_state = stepper_state.lock_inner();
                let Some(rng) = stepper_state.replay_rng().cloned() else {
                    return;
                };
                let mut rng = rng.lock();
                let offerer_rng1_state = generate_rng1_state(&mut *rng);
                let answerer_rng1_state = generate_rng1_state(&mut *rng);
                munger.set_rng1_state(
                    core,
                    if stepper_state.replay_is_offerer() {
                        offerer_rng1_state
                    } else {
                        answerer_rng1_state
                    },
                );
                munger.start_battle_from_comm_menu(
                    core,
                    bn3_match_type(&mut *rng, stepper_state.match_type()),
                    random_background(&mut *rng),
                );
            })
        }),
        (hooks.offsets.rom.round_start_ret, {
            let stepper_state = stepper_state.clone();
            Box::new(move |_core| {
                stepper_state.lock_inner().advance_to_next_replay_round_if_pending();
            })
        }),
        stepper_trap(
            hooks.offsets.rom.battle_start_play_music_call,
            &stepper_state,
            |state, mut core| {
                if !state.disable_bgm() {
                    return;
                }
                let pc = core.as_ref().gba().cpu().thumb_pc();
                core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
            },
        ),
        stepper_trap(hooks.offsets.rom.battle_is_p2_ret, &stepper_state, |state, mut core| {
            core.gba_mut().cpu_mut().set_gpr(0, state.local_player_index() as i32);
        }),
        stepper_trap(hooks.offsets.rom.link_is_p2_ret, &stepper_state, |state, mut core| {
            core.gba_mut().cpu_mut().set_gpr(0, state.local_player_index() as i32);
        }),
        stepper_trap(hooks.offsets.rom.round_ending_entry, &stepper_state, |state, _core| {
            if state.is_round_ending() {
                return;
            }
            state.set_round_ending();
        }),
        stepper_trap(hooks.offsets.rom.round_end_entry, &stepper_state, |state, _core| {
            state.set_round_ended();
        }),
        {
            let munger = hooks.munger();
            stepper_trap(
                hooks.offsets.rom.main_read_joyflags,
                &stepper_state,
                move |state, mut core| {
                    let current_tick = state.current_tick();

                    if current_tick == state.commit_tick() && !state.has_committed_this_round() && state.round_active()
                    {
                        if let Some(rng) = state.replay_rng().cloned() {
                            munger.set_rng2_state(core, generate_rng2_state(&mut *rng.lock()));
                        }
                        state.set_committed_state(core.save_state().expect("save committed state"));
                    }

                    let Some(ip) = state.peek_input_pair().cloned() else {
                        return;
                    };

                    core.gba_mut().cpu_mut().set_gpr(4, (ip.local.joyflags | 0xfc00) as i32);

                    if current_tick == state.dirty_tick() {
                        state.set_dirty_state(core.save_state().expect("save dirty state"));
                    }
                },
            )
        },
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
            hooks.offsets.rom.process_battle_input_ret,
            Box::new(move |mut core| {
                core.gba_mut().cpu_mut().set_gpr(0, 0);
            }),
        ),
        (
            hooks.offsets.rom.init_sio_call,
            Box::new(|mut core| {
                let pc = core.as_ref().gba().cpu().thumb_pc();
                core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
            }),
        ),
        stepper_trap(
            hooks.offsets.rom.round_call_jump_table_ret,
            &stepper_state,
            |state, _core| {
                state.increment_current_tick();
            },
        ),
    ];
    traps.extend(stepper_round_outcome_traps(
        &stepper_state,
        &[
            (hooks.offsets.rom.round_end_set_win, BattleOutcome::Win),
            (hooks.offsets.rom.round_end_set_loss, BattleOutcome::Loss),
            (hooks.offsets.rom.round_end_damage_judge_set_win, BattleOutcome::Win),
            (hooks.offsets.rom.round_end_damage_judge_set_loss, BattleOutcome::Loss),
            (hooks.offsets.rom.round_end_damage_judge_set_draw, BattleOutcome::Draw),
        ],
    ));
    traps
}
