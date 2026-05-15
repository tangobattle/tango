use rand::Rng;

use crate::hooks::{CompletionToken, MatchHandle, Trap};

use super::rng::generate_rng_state;
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
            let Some(_) = &*match_ else {
                core.gba_mut().cpu_mut().set_gpr(0, 0);
                return;
            };
            core.gba_mut().cpu_mut().set_gpr(0, 3);

            munger.set_rx_packet(core, 0, &INIT_RX);
            munger.set_rx_packet(core, 1, &INIT_RX);
        })
    };
    vec![
        (hooks.offsets.rom.comm_menu_init_ret, {
            let munger = hooks.munger();
            Box::new(move |core| {
                munger.start_battle_from_comm_menu(core);
            })
        }),
        (
            hooks.offsets.rom.match_end_ret,
            Box::new(move |_core| {
                completion_token.complete();
            }),
        ),
        (hooks.offsets.rom.round_ending_entry1, {
            let match_ = match_.clone();
            Box::new(move |_core| {
                let guard = match_.blocking_lock();
                let Some(match_) = guard.as_ref() else { return };
                match_.end_round().expect("end round");
            })
        }),
        (hooks.offsets.rom.round_ending_entry2, {
            let match_ = match_.clone();
            Box::new(move |_core| {
                let guard = match_.blocking_lock();
                let Some(match_) = guard.as_ref() else { return };
                match_.end_round().expect("end round");
            })
        }),
        (hooks.offsets.rom.round_start_ret, {
            let munger = hooks.munger();
            let match_ = match_.clone();
            Box::new(move |core| {
                let guard = match_.blocking_lock();
                let Some(match_) = guard.as_ref() else { return };
                crate::sync::block_on(match_.start_round()).expect("start round");
                let mut rng = match_.lock_rng();
                munger.set_battle_stage(core, rng.gen_range(0..0xc));
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

                if !round.has_committed_state() {
                    let mut rng = match_.lock_rng();
                    let offerer_rng_state = generate_rng_state(&mut *rng);
                    let answerer_rng_state = generate_rng_state(&mut *rng);
                    munger.set_rng_state(
                        core,
                        if match_.is_offerer() {
                            offerer_rng_state
                        } else {
                            answerer_rng_state
                        },
                    );

                    match_
                        .record_first_commit(round, core.save_state().expect("save state"), &munger.tx_packet(core))
                        .expect("record first commit");
                    log::info!("primary rng state: {:08x}", munger.rng_state(core));
                    log::info!("battle state committed on {}", round.current_tick());
                }

                if let Err(e) = crate::sync::block_on(round.add_local_input_and_fastforward(
                    core,
                    joyflags.load(std::sync::atomic::Ordering::Relaxed) as u16,
                )) {
                    log::error!("failed to add local input: {}", e);
                    match_.cancel();
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
