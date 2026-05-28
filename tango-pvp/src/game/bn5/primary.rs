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
                // Pre-seed rng1 (local, used for settings) and rng2
                // (shared, used for background) so the ROM generator
                // produces a peer-agreeing (settings, bg).
                let seed = generate_rng2_state(&mut *rng);
                munger.set_rng1_state(core, seed);
                munger.set_rng2_state(core, seed);
                // Advance submenu state so the next outer-dispatcher
                // tick lands at init_battle_entry, which consumes the
                // settings the handler is about to write.
                munger.select_init_battle_substate(core);
                // Skip past the function's SIO check (`bl <fn>; beq`)
                // by jumping straight to the generator-path branch.
                let pc = core.as_ref().gba().cpu().thumb_pc();
                core.gba_mut().cpu_mut().set_thumb_pc(pc + 0x12);
            })
        }),
        (
            hooks.offsets.rom.comm_menu_end_battle_entry,
            Box::new(move |_core| {
                completion_token.complete();
            }),
        ),
        (hooks.offsets.rom.in_battle_call_handle_link_cable_input, {
            let match_ = match_.clone();
            let munger = hooks.munger();
            Box::new(move |mut core| {
                let pc = core.as_ref().gba().cpu().thumb_pc();
                core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
                munger.set_copy_data_input_state(core, if match_.blocking_lock().is_some() { 2 } else { 4 });
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
                let expected = round.expected_game_tick();
                if game_current_tick != expected {
                    panic!(
                        "read joyflags: expected game tick = {} but game tick = {}",
                        expected, game_current_tick
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
                let Some(round) = round_state.as_mut() else { return };
                if !round.has_committed_state() {
                    return;
                }

                round.increment_current_tick();
                let game_current_tick = munger.current_tick(core);
                let expected = round.expected_game_tick();
                if game_current_tick != expected {
                    panic!(
                        "post increment tick: expected game tick = {} but game tick = {}",
                        expected, game_current_tick
                    );
                }
            })
        }),
    ]
}
