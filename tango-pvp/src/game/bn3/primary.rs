use byteorder::ByteOrder;

use crate::hooks::{CompletionToken, MatchHandle, Trap};

use super::rng::{bn3_match_type, generate_rng2_state, pick_rng1_state};
use super::INIT_RX;

pub(super) fn traps(
    hooks: &super::Hooks,
    joyflags: std::sync::Arc<std::sync::atomic::AtomicU32>,
    match_: MatchHandle,
    completion_token: CompletionToken,
) -> Vec<Trap> {
    let make_send_and_receive_call_hook = || {
        let match_ = match_.clone();
        let munger = hooks.munger();
        Box::new(move |mut core: mgba::core::CoreMutRef| {
            let pc = core.as_ref().gba().cpu().thumb_pc();
            core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);

            let match_ = match_.blocking_lock();
            let Some(match_) = &*match_ else {
                core.gba_mut().cpu_mut().set_gpr(0, 0);
                return;
            };
            core.gba_mut().cpu_mut().set_gpr(0, 3);

            let mut round_state = match_.lock_round_state();

            let Some(round) = round_state.as_mut() else {
                return;
            };

            let current_tick = round.current_tick();
            if current_tick > 1 {
                let mut rx = [0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
                byteorder::LittleEndian::write_u32(&mut rx[4..8], current_tick.saturating_sub(2));
                munger.set_rx_packet(core, 0, &rx);
                munger.set_rx_packet(core, 1, &rx);
            }
        })
    };

    vec![
        (hooks.offsets.rom.comm_menu_init_ret, {
            let munger = hooks.munger();
            let match_ = match_.clone();
            Box::new(move |core| {
                let guard = match_.blocking_lock();
                let Some(match_) = guard.as_ref() else { return };
                let mut rng = match_.lock_rng();

                // rng1 is the local rng, it should not be synced.
                // However, we should make sure it's reproducible from the shared RNG state so we generate it like this.
                let rng1_state = pick_rng1_state(&mut *rng, match_.is_offerer());
                munger.set_rng1_state(core, rng1_state);
                munger.start_battle_from_comm_menu(core, bn3_match_type(&mut *rng, match_.match_type()));
            })
        }),
        (hooks.offsets.rom.comm_menu_settings_entry, {
            let munger = hooks.munger();
            let match_ = match_.clone();
            Box::new(move |mut core| {
                let guard = match_.blocking_lock();
                let Some(match_) = guard.as_ref() else { return };
                let mut rng = match_.lock_rng();
                // The ROM bg generator reads rng2 (shared). Pre-seed
                // it from the synced match RNG so both peers compute
                // the same bg.
                let r2_seed = generate_rng2_state(&mut *rng);
                munger.set_rng2_state(core, r2_seed);
                // Advance to Trill's original post-handshake state so
                // once the handler returns, the next outer-dispatcher
                // tick lands at the battle-init path.
                munger.select_battle_init_substate(core, 0x30);
                // PC-redirect past the function's SIO checks and its
                // own [1]=0x34 write — landing at the BL to the bg
                // generator. The function then writes the bg into the
                // tx_packet via `strb r4, [r7, #4]`.
                let pc = core.as_ref().gba().cpu().thumb_pc();
                core.gba_mut().cpu_mut().set_thumb_pc(pc + 0x50);
            })
        }),
        (
            hooks.offsets.rom.match_end_ret,
            Box::new(move |_core| {
                completion_token.complete();
            }),
        ),
        (hooks.offsets.rom.round_ending_entry, {
            let match_ = match_.clone();
            Box::new(move |_core| {
                let guard = match_.blocking_lock();
                let Some(match_) = guard.as_ref() else { return };
                match_.end_round().expect("end round");
            })
        }),
        (hooks.offsets.rom.round_start_ret, {
            let match_ = match_.clone();
            Box::new(move |_core| {
                let guard = match_.blocking_lock();
                let Some(match_) = guard.as_ref() else { return };
                crate::sync::block_on(match_.start_round()).expect("start round");
            })
        }),
        (hooks.offsets.rom.battle_is_p2_ret, {
            let match_ = match_.clone();
            Box::new(move |mut core| {
                let guard = match_.blocking_lock();
                let Some(match_) = guard.as_ref() else { return };
                let mut round_state = match_.lock_round_state();
                let Some(round) = round_state.as_mut() else { return };
                core.gba_mut().cpu_mut().set_gpr(0, round.local_player_index() as i32);
            })
        }),
        (hooks.offsets.rom.link_is_p2_ret, {
            let match_ = match_.clone();
            Box::new(move |mut core| {
                let guard = match_.blocking_lock();
                let Some(match_) = guard.as_ref() else { return };
                let mut round_state = match_.lock_round_state();
                let Some(round) = round_state.as_mut() else { return };
                core.gba_mut().cpu_mut().set_gpr(0, round.local_player_index() as i32);
            })
        }),
        (hooks.offsets.rom.main_read_joyflags, {
            let munger = hooks.munger();
            let match_ = match_.clone();
            Box::new(move |core| {
                let guard = match_.blocking_lock();
                let Some(match_) = guard.as_ref() else { return };
                let mut round_state = match_.lock_round_state();
                let Some(round) = round_state.as_mut() else { return };

                if !munger.is_linking(core) {
                    return;
                }

                if !round.has_committed_state() {
                    let mut rng = match_.lock_rng();

                    // rng2 is the shared rng, it must be synced.
                    let rng2_state = generate_rng2_state(&mut *rng);
                    munger.set_rng2_state(core, rng2_state);

                    match_
                        .record_first_commit(round, core.save_state().expect("save state"), &munger.tx_packet(core))
                        .expect("record first commit");
                    log::info!(
                        "primary rng1 state: {:08x}, rng2 state: {:08x}",
                        munger.rng1_state(core),
                        munger.rng2_state(core),
                    );
                    log::info!("battle state committed on {}", round.current_tick());
                }

                if let Err(e) =
                    crate::sync::block_on(round.add_local_input_and_fastforward(
                        core,
                        joyflags.load(std::sync::atomic::Ordering::Relaxed) as u16,
                    ))
                {
                    log::error!("failed to add local input: {}", e);
                    match_.cancel();
                }
            })
        }),
        (
            hooks.offsets.rom.process_battle_input_ret,
            Box::new(move |mut core| {
                core.gba_mut().cpu_mut().set_gpr(0, 0);
            }),
        ),
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
            hooks.offsets.rom.init_sio_call,
            Box::new(|mut core| {
                let pc = core.as_ref().gba().cpu().thumb_pc();
                core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
            }),
        ),
        (hooks.offsets.rom.round_call_jump_table_ret, {
            let match_ = match_.clone();
            Box::new(move |_core| {
                let guard = match_.blocking_lock();
                let Some(match_) = guard.as_ref() else { return };
                let mut round_state = match_.lock_round_state();
                let Some(round) = round_state.as_mut() else { return };
                if !round.has_committed_state() {
                    return;
                }
                round.increment_current_tick();
            })
        }),
    ]
}
