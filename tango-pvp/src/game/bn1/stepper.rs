use byteorder::ByteOrder;

use crate::hooks::Trap;
use crate::stepper::BattleOutcome;

use super::rng::pick_rng_state;

pub(super) fn traps(hooks: &super::Hooks, stepper_state: crate::stepper::State) -> Vec<Trap> {
    let make_send_and_receive_call_hook = || {
        let munger = hooks.munger();
        let stepper_state = stepper_state.clone();
        Box::new(move |mut core: mgba::core::CoreMutRef| {
            let mut stepper_state = stepper_state.lock_inner();

            let pc = core.as_ref().gba().cpu().thumb_pc();
            core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
            core.gba_mut().cpu_mut().set_gpr(0, 3);

            if stepper_state.is_round_ending() {
                return;
            }
            if stepper_state.is_replaying() && !stepper_state.has_committed_this_round() {
                return;
            }

            let (local, remote) = match stepper_state.pop_input_pair() {
                Some(ip) => ip,
                None => {
                    let mut rx = [
                        0x80, 0x00, 0x00, 0xfc, 0x00, 0x00, 0x00, 0xfc, 0x00, 0xfc, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff,
                    ];
                    byteorder::LittleEndian::write_u32(&mut rx[0x0c..0x10], munger.packet_seqnum(core));
                    munger.set_rx_packet(core, 0, &rx);
                    munger.set_rx_packet(core, 1, &rx);
                    return;
                }
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
                    .apply_shadow_input((local.with_packet(local_packet), remote))
                    .expect("apply shadow input")
                    .try_into()
                    .unwrap(),
            );
            stepper_state.set_local_packet(munger.tx_packet(core).to_vec());
        })
    };

    vec![
        (hooks.offsets.rom.comm_menu_init_ret, {
            let munger = hooks.munger();
            Box::new(move |core| {
                munger.start_battle_from_comm_menu(core);
            })
        }),
        (hooks.offsets.rom.round_start_entry, {
            let munger = hooks.munger();
            let stepper_state = stepper_state.clone();
            Box::new(move |core| {
                let Some(rng) = stepper_state.lock_inner().replay_rng().cloned() else {
                    return;
                };
                let mut rng = rng.lock().unwrap();
                let rng_state = pick_rng_state(&mut *rng, stepper_state.lock_inner().replay_is_offerer());
                munger.set_rng_state(core, rng_state);
                munger.set_frame_counter(core, rng_state as u16);
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
        (hooks.offsets.rom.link_is_p2_ret, {
            let stepper_state = stepper_state.clone();
            Box::new(move |mut core: mgba::core::CoreMutRef| {
                let state = stepper_state.lock_inner();
                core.gba_mut().cpu_mut().set_gpr(0, state.local_player_index() as i32);
            })
        }),
        (hooks.offsets.rom.round_ending_entry1, {
            let stepper_state = stepper_state.clone();
            Box::new(move |_core| {
                let mut state = stepper_state.lock_inner();
                if state.is_round_ending() {
                    return;
                }
                state.set_round_ending();
            })
        }),
        (hooks.offsets.rom.round_ending_entry2, {
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
                    state.set_local_packet(munger.tx_packet(core).to_vec());
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

                core.gba_mut().cpu_mut().set_gpr(4, (local.joyflags | 0xfc00) as i32);
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
    ]
}
