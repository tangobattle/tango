use byteorder::ByteOrder;

use crate::hooks::Trap;
use crate::stepper::BattleOutcome;

use super::rng::{bn3_match_type, generate_rng2_state, pick_rng1_state};
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

            if stepper_state.is_replaying() && !stepper_state.has_committed_this_round() {
                return;
            }

            if stepper_state.is_round_ending() {
                return;
            }

            let Some((local, remote)) = stepper_state.pop_input_pair() else {
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
            // On failure the error is already on the stepper's error channel;
            // the drive loop aborts on it.
            let Some(remote_packet) = stepper_state.apply_shadow_input((local.with_packet(local_packet), remote))
            else {
                return;
            };
            munger.set_rx_packet(
                core,
                stepper_state.remote_player_index() as u32,
                &remote_packet.try_into().unwrap(),
            );
            stepper_state.set_local_packet(munger.tx_packet(core).to_vec());
        })
    };

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
                let mut stepper_state = stepper_state.lock_inner();
                let is_offerer = stepper_state.replay_is_offerer();
                let match_type = stepper_state.match_type();
                let Some(rng) = stepper_state.replay_rng_mut() else {
                    return;
                };
                let rng1_state = pick_rng1_state(rng, is_offerer);
                munger.set_rng1_state(core, rng1_state);
                munger.start_battle_from_comm_menu(core, bn3_match_type(rng, match_type));
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
                let r2_seed = generate_rng2_state(rng);
                munger.set_rng2_state(core, r2_seed);
                munger.select_battle_init_substate(core, 0x30);
                let pc = core.as_ref().gba().cpu().thumb_pc();
                core.gba_mut().cpu_mut().set_thumb_pc(pc + 0x50);
            })
        }),
        (hooks.offsets.rom.round_start_ret, {
            let stepper_state = stepper_state.clone();
            Box::new(move |_core| {
                let mut state = stepper_state.lock_inner();
                state.advance_to_next_replay_round_if_pending();
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
        (hooks.offsets.rom.battle_is_p2_ret, make_is_p2_hook()),
        (hooks.offsets.rom.link_is_p2_ret, make_is_p2_hook()),
        (hooks.offsets.rom.round_ending_entry, {
            let stepper_state = stepper_state.clone();
            Box::new(move |_core| {
                let mut state = stepper_state.lock_inner();
                if state.is_round_ending() {
                    return;
                }
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
                // scene, not just battle. Without it the stepper would inject
                // recorded battle joyflags into r4 during pre-battle code (bn2
                // saw this corrupt the comm-menu state machine).
                if state.is_replaying() && !state.round_active() {
                    return;
                }
                // Replay-mode-only first-commit hook; never fires in FF mode.
                if state.needs_replay_first_commit() {
                    if let Some(rng) = state.replay_rng_mut() {
                        let rng2_state = generate_rng2_state(rng);
                        munger.set_rng2_state(core, rng2_state);
                    }
                    state.set_local_packet(munger.tx_packet(core).to_vec());
                    state.on_first_commit();
                }

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
        (hooks.offsets.rom.sio_teardown_clear_entry, {
            Box::new(|mut core| {
                // Skip the 3-instruction clear block plus the trailing
                // SIO-register-cleanup BL, landing on the function's
                // pop {pc}. Stack stays balanced because the function's
                // push {lr} already executed.
                let pc = core.as_ref().gba().cpu().thumb_pc();
                core.gba_mut().cpu_mut().set_thumb_pc(pc + 0xc);
            })
        }),
        (hooks.offsets.rom.comm_status_check_entry, {
            Box::new(|mut core| {
                // Force return value 0 (no error) and PC-redirect to
                // the function's `mov pc, lr` epilogue. Defeats the
                // state-1 → state-3 transition that displays the
                // "Communication Error" UI.
                let pc = core.as_ref().gba().cpu().thumb_pc();
                core.gba_mut().cpu_mut().set_gpr(0, 0);
                core.gba_mut().cpu_mut().set_thumb_pc(pc + 0x22);
            })
        }),
        (hooks.offsets.rom.round_call_jump_table_ret, {
            let stepper_state = stepper_state.clone();
            Box::new(move |_core| {
                let mut state = stepper_state.lock_inner();
                state.increment_current_tick();
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
