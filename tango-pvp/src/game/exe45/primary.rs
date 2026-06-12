use crate::hooks::{CompletionToken, MatchHandle, Trap};

use super::rng::{generate_rng2_state, pick_rng_states};

pub(super) fn traps(
    hooks: &super::Hooks,
    joyflags: std::sync::Arc<std::sync::atomic::AtomicU32>,
    match_: MatchHandle,
    completion_token: CompletionToken,
    disable_bgm: bool,
) -> Vec<Trap> {
    // The game asks "am I player 2?" at two ROM sites; both get the same hook.
    let make_is_p2_hook = || {
        let match_ = match_.clone();
        Box::new(move |mut core: mgba::core::CoreMutRef| {
            let Some(match_) = match_.get() else { return };
            let mut round_state = match_.lock_round_state();
            let Some(round) = round_state.as_mut() else { return };
            core.gba_mut().cpu_mut().set_gpr(0, round.local_player_index() as i32);
        })
    };

    vec![
        (
            hooks.offsets.rom.battle_start_play_music_call,
            Box::new(move |mut core: mgba::core::CoreMutRef| {
                if !disable_bgm {
                    return;
                }
                let pc = core.as_ref().gba().cpu().thumb_pc();
                core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
            }),
        ),
        (hooks.offsets.rom.comm_menu_init_ret, {
            let munger = hooks.munger();
            let match_ = match_.clone();
            Box::new(move |core| {
                let Some(match_) = match_.get() else { return };
                munger.start_battle_from_comm_menu(core, match_.match_type().0);
            })
        }),
        (hooks.offsets.rom.comm_menu_settings_entry, {
            let munger = hooks.munger();
            let match_ = match_.clone();
            Box::new(move |core| {
                let Some(match_) = match_.get() else { return };
                let mut rng = match_.lock_rng();
                let seed = generate_rng2_state(&mut *rng);
                munger.set_rng1_state(core, seed);
                munger.set_rng2_state(core, seed);
            })
        }),
        (
            hooks.offsets.rom.match_end_ret,
            Box::new(move |_core| {
                completion_token.complete();
            }),
        ),
        (hooks.offsets.rom.round_set_ending, {
            let match_ = match_.clone();
            Box::new(move |_core| {
                let Some(match_) = match_.get() else { return };
                match_.end_round_or_cancel();
            })
        }),
        (hooks.offsets.rom.round_start_ret, {
            let match_ = match_.clone();
            Box::new(move |_core| {
                let Some(match_) = match_.get() else { return };
                match_.start_round_or_cancel();
            })
        }),
        (hooks.offsets.rom.battle_is_p2_tst, make_is_p2_hook()),
        (hooks.offsets.rom.link_is_p2_ret, make_is_p2_hook()),
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
                munger.set_copy_data_input_state(core, if match_.is_set() { 2 } else { 4 });
            })
        }),
        (hooks.offsets.rom.main_read_joyflags, {
            let munger = hooks.munger();
            let match_ = match_.clone();
            Box::new(move |core| {
                let Some(match_) = match_.get() else { return };
                let mut round_state = match_.lock_round_state();
                let Some(round) = round_state.as_mut() else { return };

                if !round.has_settled_snapshot() {
                    let mut rng = match_.lock_rng();

                    // rng1 is the local rng, it should not be synced.
                    // However, we should make sure it's reproducible from the shared RNG state so we generate it like this.
                    // rng2 is the shared rng, it must be synced.
                    let (rng1_state, rng2_state) = pick_rng_states(&mut *rng, match_.is_offerer());
                    munger.set_rng1_state(core, rng1_state);
                    munger.set_rng2_state(core, rng2_state);

                    if let Err(e) = match_.record_first_commit(round, core, &munger.tx_packet(core)) {
                        log::error!("record first commit failed: {e:#}");
                        match_.cancel();
                        return;
                    }
                    log::info!(
                        "primary rng1 state: {:08x}, rng2 state: {:08x}",
                        munger.rng1_state(core),
                        munger.rng2_state(core),
                    );
                }

                if let Err(e) =
                    crate::sync::block_on(round.add_local_input_and_fastforward(
                        match_.sender(),
                        core,
                        joyflags.load(std::sync::atomic::Ordering::Relaxed) as u16,
                    ))
                {
                    log::error!("failed to add local input: {}", e);
                    match_.cancel();
                }
            })
        }),
    ]
}
