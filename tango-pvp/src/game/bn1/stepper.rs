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
            // FF has captured its state and the Fastforwarder is about to
            // return. `run_loop`'s remaining cycle budget can still spill
            // past the trap-fire point — don't let it advance the shadow
            // again for the captured tick.
            if stepper_state.has_captured_state() {
                return;
            }

            let ip = match stepper_state.pop_input_pair() {
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
                let current_tick = state.current_tick();

                // Replay-mode-only first-commit hook. FF mode bypasses this
                // (commit_frontier = u32::MAX there).
                if state.is_replaying()
                    && current_tick == state.commit_frontier()
                    && !state.has_committed_this_round()
                    && state.round_active()
                {
                    state.set_local_packet(munger.tx_packet(core).to_vec());
                    state.on_first_commit();
                }

                let Some(ip) = state.peek_input_pair().cloned() else {
                    return;
                };

                core.gba_mut().cpu_mut().set_gpr(4, (ip.local.joyflags | 0xfc00) as i32);

                // FF state capture (post-peek so r4 is set). `capture_tick` is
                // u32::MAX in replay mode, so this never fires there.
                if current_tick == state.capture_tick() {
                    state.set_local_packet(munger.tx_packet(core).to_vec());
                    state.set_captured_state(core.save_state().expect("save captured state"));
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
