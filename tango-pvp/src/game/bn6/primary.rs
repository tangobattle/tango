use crate::hooks::{CompletionToken, MatchHandle, Trap};

use super::rng::{generate_rng2_state, pick_rng_states};

pub(super) fn traps(
    hooks: &super::Hooks,
    joyflags: std::sync::Arc<std::sync::atomic::AtomicU32>,
    match_: MatchHandle,
    completion_token: CompletionToken,
) -> Vec<Trap> {
    vec![
        (hooks.offsets.rom.comm_menu_init_ret, {
            let munger = hooks.munger();
            let match_ = match_.clone();
            Box::new(move |core| {
                let guard = match_.blocking_lock();
                let Some(match_) = guard.as_ref() else { return };
                munger.start_battle_from_comm_menu(core, match_.match_type().0);
            })
        }),
        (hooks.offsets.rom.round_set_ending, {
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
        (hooks.offsets.rom.battle_is_p2_tst, {
            let match_ = match_.clone();
            Box::new(move |mut core| {
                let guard = match_.blocking_lock();
                let Some(match_) = guard.as_ref() else { return };
                let mut round_state = match_.lock_round_state();
                let Some(round) = round_state.as_mut() else {
                    return;
                };
                core.gba_mut().cpu_mut().set_gpr(0, round.local_player_index() as i32);
            })
        }),
        (hooks.offsets.rom.link_is_p2_ret, {
            let match_ = match_.clone();
            Box::new(move |mut core| {
                let guard = match_.blocking_lock();
                let Some(match_) = guard.as_ref() else { return };
                let mut round_state = match_.lock_round_state();
                let Some(round) = round_state.as_mut() else {
                    return;
                };
                core.gba_mut().cpu_mut().set_gpr(0, round.local_player_index() as i32);
            })
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
        (hooks.offsets.rom.comm_menu_settings_entry, {
            let munger = hooks.munger();
            let match_ = match_.clone();
            Box::new(move |mut core| {
                let guard = match_.blocking_lock();
                let Some(match_) = guard.as_ref() else { return };
                let mut rng = match_.lock_rng();
                let seed = generate_rng2_state(&mut *rng);
                munger.set_rng1_state(core, seed);
                munger.set_rng2_state(core, seed);
                munger.select_battle_init_substate(core, 0x18);
                let pc = core.as_ref().gba().cpu().thumb_pc();
                core.gba_mut().cpu_mut().set_thumb_pc(pc + 0xa6);
            })
        }),
        (
            hooks.offsets.rom.comm_menu_end_battle_entry,
            Box::new(move |_core| {
                completion_token.complete();
            }),
        ),
        (
            hooks
                .offsets
                .rom
                .comm_menu_in_battle_call_comm_menu_handle_link_cable_input,
            {
                let match_ = match_.clone();
                let munger = hooks.munger();
                Box::new(move |mut core| {
                    let pc = core.as_ref().gba().cpu().thumb_pc();
                    core.gba_mut().cpu_mut().set_thumb_pc(pc + 6);
                    munger.set_copy_data_input_state(core, if match_.blocking_lock().is_some() { 2 } else { 4 });
                })
            },
        ),
        (hooks.offsets.rom.main_read_joyflags, {
            let munger = hooks.munger();
            let match_ = match_.clone();
            Box::new(move |core| {
                let guard = match_.blocking_lock();
                let Some(match_) = guard.as_ref() else { return };
                let mut round_state = match_.lock_round_state();
                let Some(round) = round_state.as_mut() else {
                    return;
                };

                if !round.has_committed_state() {
                    let mut rng = match_.lock_rng();

                    // rng1 is the local rng, it should not be synced.
                    // However, we should make sure it's reproducible from the shared RNG state so we generate it like this.
                    // rng2 is the shared rng, it must be synced.
                    let (rng1_state, rng2_state) = pick_rng_states(&mut *rng, match_.is_offerer());
                    munger.set_rng1_state(core, rng1_state);
                    munger.set_rng2_state(core, rng2_state);

                    // HACK: The battle jump table goes directly from deinit to init, so we actually end up initializing on tick 1 after round 1. We just override it here.
                    munger.set_current_tick(core, 0);

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

                let game_current_tick = munger.current_tick(core);
                if game_current_tick != round.current_tick() {
                    panic!(
                        "read joyflags: round tick = {} but game tick = {}",
                        round.current_tick(),
                        game_current_tick
                    );
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
        (hooks.offsets.rom.round_post_increment_tick, {
            let munger = hooks.munger();
            let match_ = match_.clone();
            Box::new(move |core| {
                let guard = match_.blocking_lock();
                let Some(match_) = guard.as_ref() else { return };
                let mut round_state = match_.lock_round_state();
                let Some(round) = round_state.as_mut() else {
                    return;
                };
                if !round.has_committed_state() {
                    return;
                }

                round.increment_current_tick();
                let game_current_tick = munger.current_tick(core);
                if game_current_tick != round.current_tick() {
                    panic!(
                        "post increment tick: round tick = {} but game tick = {}",
                        round.current_tick(),
                        game_current_tick
                    );
                }
            })
        }),
    ]
}
