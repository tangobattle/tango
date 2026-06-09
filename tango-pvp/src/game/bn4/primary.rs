use crate::hooks::{CompletionToken, MatchHandle, Trap};

use super::rng::{generate_rng2_state, pick_rng_states};

pub(super) fn traps(
    hooks: &super::Hooks,
    joyflags: std::sync::Arc<std::sync::atomic::AtomicU32>,
    match_: MatchHandle,
    completion_token: CompletionToken,
) -> Vec<Trap> {
    let mut traps: Vec<Trap> =
        vec![
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
                Box::new(move |mut core| {
                    let Some(match_) = match_.get() else { return };
                    let mut rng = match_.lock_rng();
                    let seed = generate_rng2_state(&mut *rng);
                    munger.set_rng1_state(core, seed);
                    munger.set_rng2_state(core, seed);
                    // Advance to Tango's battle-init state so once the
                    // settings handler returns and the state machine
                    // ticks again, the outer dispatcher routes to the
                    // battle-init path (which reads the [0x11]/[0x2c]
                    // the handler is about to write).
                    munger.select_battle_init_substate(core);
                    // PC-redirect past the function's SIO/button check
                    // *and* its own [2]=0xc / [3]=0 writes (which would
                    // undo the substate we just set) — landing right
                    // before `bl 0x803b23c` so the generator path runs.
                    let pc = core.as_ref().gba().cpu().thumb_pc();
                    core.gba_mut().cpu_mut().set_thumb_pc(pc + 0x62);
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
                    match_.end_round().expect("end round");
                })
            }),
            (hooks.offsets.rom.round_start_ret, {
                let match_ = match_.clone();
                Box::new(move |_core| {
                    let Some(match_) = match_.get() else { return };
                    crate::sync::block_on(match_.start_round()).expect("start round");
                })
            }),
            (hooks.offsets.rom.battle_is_p2_tst, {
                let match_ = match_.clone();
                Box::new(move |mut core| {
                    let Some(match_) = match_.get() else { return };
                    let mut round_state = match_.lock_round_state();
                    let Some(round) = round_state.as_mut() else { return };
                    core.gba_mut().cpu_mut().set_gpr(0, round.local_player_index() as i32);
                })
            }),
            (hooks.offsets.rom.link_is_p2_ret, {
                let match_ = match_.clone();
                Box::new(move |mut core| {
                    let Some(match_) = match_.get() else { return };
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
            (hooks.offsets.rom.in_battle_call_handle_link_cable_input, {
                let match_ = match_.clone();
                let munger = hooks.munger();
                Box::new(move |mut core| {
                    let pc = core.as_ref().gba().cpu().thumb_pc();
                    core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
                    munger.set_copy_data_input_state(core, if match_.get().is_some() { 2 } else { 4 });
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

                        match_
                            .record_first_commit(round, core.save_state().expect("save state"), &munger.tx_packet(core))
                            .expect("record first commit");
                        log::info!(
                            "primary rng1 state: {:08x}, rng2 state: {:08x}",
                            munger.rng1_state(core),
                            munger.rng2_state(core),
                        );
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
        ];
    traps.extend(super::pizzazz::primary_traps(hooks, &match_));
    traps
}
