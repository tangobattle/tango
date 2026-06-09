use crate::hooks::Trap;

use super::rng::{generate_rng2_state, pick_rng_states};

pub(super) fn traps(hooks: &super::Hooks, shadow_state: crate::shadow::State) -> Vec<Trap> {
    vec![
        (hooks.offsets.rom.comm_menu_init_ret, {
            let munger = hooks.munger();
            let shadow_state = shadow_state.clone();
            Box::new(move |core| {
                munger.start_battle_from_comm_menu(core, shadow_state.match_type().0);
            })
        }),
        (hooks.offsets.rom.round_start_ret, {
            let shadow_state = shadow_state.clone();
            Box::new(move |_core| {
                shadow_state.start_round();
            })
        }),
        (hooks.offsets.rom.round_end_entry, {
            let shadow_state = shadow_state.clone();
            Box::new(move |mut core| {
                shadow_state.end_round();
                // Round's over — halt run_loop here so it can't spill into the
                // inter-round transition.
                core.end_run_loop();
            })
        }),
        (hooks.offsets.rom.battle_is_p2_tst, {
            let shadow_state = shadow_state.clone();
            Box::new(move |mut core| {
                let mut round_state = shadow_state.lock_round_state();
                let Some(round) = round_state.round.as_mut() else {
                    return;
                };
                core.gba_mut().cpu_mut().set_gpr(0, round.remote_player_index() as i32);
            })
        }),
        (hooks.offsets.rom.link_is_p2_ret, {
            let shadow_state = shadow_state.clone();
            Box::new(move |mut core| {
                let mut round_state = shadow_state.lock_round_state();
                let Some(round) = round_state.round.as_mut() else {
                    return;
                };
                core.gba_mut().cpu_mut().set_gpr(0, round.remote_player_index() as i32);
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
            let shadow_state = shadow_state.clone();
            Box::new(move |mut core| {
                let mut rng = shadow_state.lock_rng();
                let seed = generate_rng2_state(&mut *rng);
                munger.set_rng1_state(core, seed);
                munger.set_rng2_state(core, seed);
                munger.select_battle_init_substate(core, 0x18);
                let pc = core.as_ref().gba().cpu().thumb_pc();
                core.gba_mut().cpu_mut().set_thumb_pc(pc + 0xa6);
            })
        }),
        {
            let munger = hooks.munger();
            (
                hooks
                    .offsets
                    .rom
                    .comm_menu_in_battle_call_comm_menu_handle_link_cable_input,
                Box::new(move |mut core| {
                    let pc = core.as_ref().gba().cpu().thumb_pc();
                    core.gba_mut().cpu_mut().set_thumb_pc(pc + 6);
                    munger.set_copy_data_input_state(core, 2);
                }),
            )
        },
        (hooks.offsets.rom.main_read_joyflags, {
            let munger = hooks.munger();
            let shadow_state = shadow_state.clone();
            // Timer lives in the closure (RefCell for &mut): the shadow's
            // per-round state is cloned for snapshots, so it can't hold the
            // (non-Clone) timer. Enabled only when the match set a limit (the
            // shadow's `InnerState` carries it; replay reconstruction = None, so
            // the golden suite stays byte-identical).
            let custom_timer: std::cell::RefCell<Option<crate::custom_screen::CustomScreenTimer>> =
                std::cell::RefCell::new(None);
            Box::new(move |mut core| {
                let mut round_state = shadow_state.lock_round_state();
                let Some(round) = round_state.round.as_mut() else {
                    return;
                };

                if !round.has_first_committed_state() {
                    let mut rng = shadow_state.lock_rng();

                    // rng1 is the local rng, it should not be synced.
                    // However, we should make sure it's reproducible from the shared RNG state so we generate it like this.
                    // rng2 is the shared rng, it must be synced.
                    let (rng1_state, rng2_state) = pick_rng_states(&mut *rng, !shadow_state.is_offerer());
                    munger.set_rng1_state(core, rng1_state);
                    munger.set_rng2_state(core, rng2_state);

                    // HACK: The battle jump table goes directly from deinit to init, so we actually end up initializing on tick 1 after round 1. We just override it here.
                    munger.set_current_tick(core, 0);

                    round.set_first_committed(&munger.tx_packet(core));
                    // Halt run_loop at the first committed tick so it can't over-run it.
                    core.end_run_loop();
                    log::info!(
                        "shadow rng1 state: {:08x}, rng2 state: {:08x}",
                        munger.rng1_state(core),
                        munger.rng2_state(core)
                    );
                    log::info!("shadow state committed on {}", round.current_tick());
                    return;
                }

                // Chip-select cap: pin the remote peer's screen onto the confirm
                // path so the shadow predicts the remote's forced close instead
                // of mispredicting it (which would cause rollback churn). The A
                // confirm rides the remote's synced input.
                if let Some(limit) = shadow_state.custom_screen_tick_limit() {
                    custom_timer
                        .borrow_mut()
                        .get_or_insert_with(|| {
                            crate::custom_screen::CustomScreenTimer::new(Box::new(munger.clone()), limit)
                        })
                        .enforce(core, round.current_tick());
                }

                let game_current_tick = munger.current_tick(core);
                if game_current_tick != round.current_tick() {
                    shadow_state.set_anyhow_error(anyhow::anyhow!(
                        "read joyflags: round tick = {} but game tick = {}",
                        round.current_tick(),
                        game_current_tick
                    ));
                }

                if let Some(pending) = round.peek_shadow_input() {
                    let (_local, remote) = &pending.pair;
                    core.gba_mut()
                        .cpu_mut()
                        .set_gpr(4, (remote.joyflags | 0xfc00) as i32);
                }

                if round.take_input_injected() {
                    // The input's been applied and the core has reached the next
                    // tick's read_joyflags; signal apply_input and stop here so
                    // run_loop parks the shadow exactly at that boundary.
                    shadow_state.set_input_applied();
                    core.end_run_loop();
                }
            })
        }),
        (hooks.offsets.rom.copy_input_data_entry, {
            let munger = hooks.munger();
            let shadow_state = shadow_state.clone();
            Box::new(move |core| {
                let mut round_state = shadow_state.lock_round_state();
                let Some(round) = round_state.round.as_mut() else {
                    return;
                };
                let game_current_tick = munger.current_tick(core);
                if game_current_tick != round.current_tick() {
                    shadow_state.set_anyhow_error(anyhow::anyhow!(
                        "copy input data: round tick = {} but game tick = {}",
                        round.current_tick(),
                        game_current_tick
                    ));
                }

                let Some(pending) = round.take_shadow_input() else {
                    return;
                };
                let (local, _remote) = pending.pair;

                if let Err(e) = round.check_remote_packet_at_current_tick() {
                    shadow_state.set_anyhow_error(e);
                    return;
                }

                let remote_packet = round.peek_remote_packet().unwrap();

                munger.set_rx_packet(
                    core,
                    round.local_player_index() as u32,
                    &local.packet.try_into().unwrap(),
                );
                munger.set_rx_packet(
                    core,
                    round.remote_player_index() as u32,
                    &remote_packet.clone().try_into().unwrap(),
                );
            })
        }),
        (hooks.offsets.rom.copy_input_data_ret, {
            let munger = hooks.munger();
            let shadow_state = shadow_state.clone();
            Box::new(move |core| {
                let mut round_state = shadow_state.lock_round_state();
                let Some(round) = round_state.round.as_mut() else {
                    return;
                };
                let game_current_tick = munger.current_tick(core);
                if game_current_tick != round.current_tick() {
                    shadow_state.set_anyhow_error(anyhow::anyhow!(
                        "copy input data: round tick = {} but game tick = {}",
                        round.current_tick(),
                        game_current_tick
                    ));
                }

                round.set_remote_packet(munger.tx_packet(core).to_vec());
                round.set_input_injected();
            })
        }),
        (hooks.offsets.rom.round_post_increment_tick, {
            let munger = hooks.munger();
            let shadow_state = shadow_state.clone();
            Box::new(move |core| {
                let mut round_state = shadow_state.lock_round_state();
                let Some(round) = round_state.round.as_mut() else {
                    return;
                };
                if !round.has_first_committed_state() {
                    return;
                }
                round.increment_current_tick();

                let game_current_tick = munger.current_tick(core);
                if game_current_tick != round.current_tick() {
                    shadow_state.set_anyhow_error(anyhow::anyhow!(
                        "post increment tick: round tick = {} but game tick = {}",
                        round.current_tick(),
                        game_current_tick
                    ));
                }
            })
        }),
        (hooks.offsets.rom.round_end_set_win, {
            let shadow_state = shadow_state.clone();
            Box::new(move |_core| {
                shadow_state.lock_round_state().set_result_is_in();
            })
        }),
        (hooks.offsets.rom.round_end_set_loss, {
            let shadow_state = shadow_state.clone();
            Box::new(move |_core| {
                shadow_state.lock_round_state().set_result_is_in();
            })
        }),
        (hooks.offsets.rom.round_end_damage_judge_set_win, {
            let shadow_state = shadow_state.clone();
            Box::new(move |_core| {
                shadow_state.lock_round_state().set_result_is_in();
            })
        }),
        (hooks.offsets.rom.round_end_damage_judge_set_loss, {
            let shadow_state = shadow_state.clone();
            Box::new(move |_core| {
                shadow_state.lock_round_state().set_result_is_in();
            })
        }),
        (hooks.offsets.rom.round_end_damage_judge_set_draw, {
            let shadow_state = shadow_state.clone();
            Box::new(move |_core| {
                shadow_state.lock_round_state().set_result_is_in();
            })
        }),
    ]
}
