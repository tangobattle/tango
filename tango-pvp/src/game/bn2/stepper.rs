use byteorder::ByteOrder;

use crate::hooks::Trap;
use crate::stepper::BattleOutcome;

use super::rng::{generate_rng_state, pick_rng_state};
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

            let Some(ip) = stepper_state.pop_input_pair() else {
                let mut rx = [
                    0x05, 0x00, 0x00, 0xfc, 0x00, 0x00, 0x00, 0xfc, 0x00, 0xfc, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff,
                ];
                byteorder::LittleEndian::write_u32(&mut rx[0x0c..0x10], munger.packet_seqnum(core));
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

    vec![
        (hooks.offsets.rom.comm_menu_init_ret, {
            let munger = hooks.munger();
            let stepper_state = stepper_state.clone();
            Box::new(move |core| {
                let stepper_state = stepper_state.lock_inner();
                let Some(rng) = stepper_state.replay_rng().cloned() else {
                    return;
                };
                let mut rng = rng.lock().unwrap();
                let non_shared_rng_state = pick_rng_state(&mut *rng, stepper_state.replay_is_offerer());
                munger.set_rng_state(core, non_shared_rng_state);
                munger.start_battle_from_comm_menu(core);
            })
        }),
        (hooks.offsets.rom.comm_menu_settings_entry, {
            let munger = hooks.munger();
            let stepper_state = stepper_state.clone();
            Box::new(move |mut core| {
                let stepper_state = stepper_state.lock_inner();
                let Some(rng) = stepper_state.replay_rng().cloned() else {
                    return;
                };
                let mut rng = rng.lock().unwrap();
                let shared_rng_seed = generate_rng_state(&mut *rng);
                munger.set_rng_state(core, shared_rng_seed);
                munger.select_battle_init_substate(core, 0x2c);
                let pc = core.as_ref().gba().cpu().thumb_pc();
                core.gba_mut().cpu_mut().set_thumb_pc(pc + 0x70);
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
        (hooks.offsets.rom.match_end_ret, {
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
                // In replay mode, do nothing until round_start_ret has fired.
                // Primary's is_linking guard naturally skips this PC during
                // the comm-menu→battle transition; without the round_active
                // gate the stepper would inject recorded battle joyflags into
                // r4 during pre-battle code, corrupting the menu state machine.
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
                    // Mirror primary: re-seed rng_state at first commit. Primary
                    // sets it once at comm_menu_init_ret (used during init) and
                    // again here (used in-battle). Without this second seed in
                    // replay, the in-battle shared rng diverges from recording.
                    if let Some(rng) = state.replay_rng().cloned() {
                        let shared_rng_state = generate_rng_state(&mut *rng.lock().unwrap());
                        munger.set_rng_state(core, shared_rng_state);
                    }
                    // v0x18 replay stores joyflags only; seed local_packet
                    // from the game's tx_packet (set by the comm-menu bg-gen
                    // path) so the upcoming send/receive trap has a value to
                    // inject into rx[local].
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
        {
            let munger = hooks.munger();
            (
                hooks.offsets.rom.comm_menu_send_and_receive_call,
                Box::new(move |mut core| {
                    let pc = core.as_ref().gba().cpu().thumb_pc();
                    core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
                    core.gba_mut().cpu_mut().set_gpr(0, 3);
                    let tx = munger.tx_packet(core);
                    let mut rx = INIT_RX;
                    rx[2] = tx[2];
                    munger.set_rx_packet(core, 0, &rx);
                    munger.set_rx_packet(core, 1, &rx);
                }),
            )
        },
        (hooks.offsets.rom.round_call_jump_table_ret, {
            let stepper_state = stepper_state.clone();
            Box::new(move |_core| {
                let mut state = stepper_state.lock_inner();
                // Mirror primary: don't tick before first-commit. This PC fires
                // between main_read_joyflags and handle_input_*_send_and_receive
                // on the first battle tick, so without the gate current_tick
                // jumps to 1 before the first send/receive's local_packet check
                // runs, breaking the tick alignment for the whole replay.
                if state.is_replaying() && !state.has_committed_this_round() {
                    return;
                }
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
        (hooks.offsets.rom.round_end_damage_judge_set_win, {
            let stepper_state = stepper_state.clone();
            Box::new(move |_core| {
                let mut state = stepper_state.lock_inner();
                state.set_round_result(BattleOutcome::Win);
            })
        }),
        (hooks.offsets.rom.round_end_damage_judge_set_loss, {
            let stepper_state = stepper_state.clone();
            Box::new(move |_core| {
                let mut state = stepper_state.lock_inner();
                state.set_round_result(BattleOutcome::Loss);
            })
        }),
        (hooks.offsets.rom.round_end_damage_judge_set_draw, {
            let stepper_state = stepper_state.clone();
            Box::new(move |_core| {
                let mut state = stepper_state.lock_inner();
                state.set_round_result(BattleOutcome::Draw);
            })
        }),
    ]
}
