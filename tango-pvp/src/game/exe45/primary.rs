use crate::hooks::{match_round_trap, match_trap, CompletionToken, MatchHandle, Trap};

use super::rng::{generate_rng1_state, generate_rng2_state, random_battle_settings_and_background};

pub(super) fn traps(
    hooks: &super::Hooks,
    joyflags: std::sync::Arc<std::sync::atomic::AtomicU32>,
    match_: MatchHandle,
    completion_token: CompletionToken,
) -> Vec<Trap> {
    vec![
        {
            let munger = hooks.munger();
            match_trap(hooks.offsets.rom.comm_menu_init_ret, &match_, move |match_, core| {
                let mut rng = match_.lock_rng();

                let match_type = match_.match_type().0 as u32;
                let settings_and_bg = munger.get_setting_and_background_count(core, match_type);

                let (battle_settings, background) =
                    random_battle_settings_and_background(&mut *rng, settings_and_bg.0, settings_and_bg.1);

                munger.start_battle_from_comm_menu(core, match_.match_type().0, battle_settings, background);
            })
        },
        (
            hooks.offsets.rom.match_end_ret,
            Box::new(move |_core| {
                completion_token.complete();
            }),
        ),
        match_trap(hooks.offsets.rom.round_set_ending, &match_, |match_, _core| {
            match_.end_round().expect("end round");
        }),
        match_trap(hooks.offsets.rom.round_start_ret, &match_, |match_, _core| {
            crate::sync::block_on(match_.start_round()).expect("start round");
        }),
        match_round_trap(hooks.offsets.rom.battle_is_p2_tst, &match_, |_match_, round, mut core| {
            core.gba_mut().cpu_mut().set_gpr(0, round.local_player_index() as i32);
        }),
        match_round_trap(hooks.offsets.rom.link_is_p2_ret, &match_, |_match_, round, mut core| {
            core.gba_mut().cpu_mut().set_gpr(0, round.local_player_index() as i32);
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
        (hooks.offsets.rom.in_battle_call_handle_link_cable_input, {
            let match_ = match_.clone();
            let munger = hooks.munger();
            Box::new(move |mut core| {
                let pc = core.as_ref().gba().cpu().thumb_pc();
                core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
                munger.set_copy_data_input_state(core, if match_.blocking_lock().is_some() { 2 } else { 4 });
            })
        }),
        {
            let munger = hooks.munger();
            match_round_trap(hooks.offsets.rom.main_read_joyflags, &match_, move |match_, round, core| {
                if !round.has_committed_state() {
                    let mut rng = match_.lock_rng();

                    // rng1 is the local rng, it should not be synced.
                    // However, we should make sure it's reproducible from the shared RNG state so we generate it like this.
                    let offerer_rng1_state = generate_rng1_state(&mut *rng);
                    let answerer_rng1_state = generate_rng1_state(&mut *rng);
                    munger.set_rng1_state(
                        core,
                        if match_.is_offerer() {
                            offerer_rng1_state
                        } else {
                            answerer_rng1_state
                        },
                    );

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

                if let Err(e) = crate::sync::block_on(round.add_local_input_and_fastforward(
                    core,
                    joyflags.load(std::sync::atomic::Ordering::Relaxed) as u16,
                )) {
                    log::error!("failed to add local input: {}", e);
                    match_.cancel();
                }
            })
        },
        match_round_trap(hooks.offsets.rom.round_call_jump_table_ret, &match_, |_match_, round, _core| {
            if !round.has_committed_state() {
                return;
            }
            round.increment_current_tick();
        }),
    ]
}
