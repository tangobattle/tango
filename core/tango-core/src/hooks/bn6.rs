use crate::{battle, facade, hooks, input, replayer, shadow};

mod munger;
mod offsets;

#[derive(Clone)]
pub struct BN6 {
    offsets: offsets::Offsets,
    munger: munger::Munger,
}

lazy_static! {
    pub static ref MEGAMAN6_FXXBR6E_00: Box<dyn hooks::Hooks + Send + Sync> =
        BN6::new(offsets::MEGAMAN6_FXXBR6E_00);
    pub static ref MEGAMAN6_GXXBR5E_00: Box<dyn hooks::Hooks + Send + Sync> =
        BN6::new(offsets::MEGAMAN6_GXXBR5E_00);
    pub static ref ROCKEXE6_RXXBR6J_00: Box<dyn hooks::Hooks + Send + Sync> =
        BN6::new(offsets::ROCKEXE6_RXXBR6J_00);
    pub static ref ROCKEXE6_GXXBR5J_00: Box<dyn hooks::Hooks + Send + Sync> =
        BN6::new(offsets::ROCKEXE6_GXXBR5J_00);
}

impl BN6 {
    pub fn new(offsets: offsets::Offsets) -> Box<dyn hooks::Hooks + Send + Sync> {
        Box::new(BN6 {
            offsets,
            munger: munger::Munger { offsets },
        })
    }
}

fn generate_rng1_state(rng: &mut impl rand::Rng) -> u32 {
    let mut rng1_state = 0;
    for _ in 0..rng.gen_range(0..0x10000) {
        rng1_state = step_rng(rng1_state);
    }
    rng1_state
}

fn generate_rng2_state(rng: &mut impl rand::Rng) -> u32 {
    let mut rng2_state = 0xa338244f;
    for _ in 0..rng.gen_range(0..0x10000) {
        rng2_state = step_rng(rng2_state);
    }
    rng2_state
}

fn random_battle_settings_and_background(rng: &mut impl rand::Rng, match_type: u8) -> u16 {
    const BATTLE_BACKGROUNDS: &[u16] = &[
        0x00, 0x01, 0x01, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        0x0f, 0x10, 0x11, 0x11, 0x13, 0x13,
    ];

    let lo = match match_type {
        0 => rng.gen_range(0..0x44u16),
        1 => rng.gen_range(0..0x60u16),
        2 => rng.gen_range(0..0x44u16) + 0x60u16,
        _ => 0u16,
    };

    let hi = BATTLE_BACKGROUNDS[rng.gen_range(0..BATTLE_BACKGROUNDS.len())];

    hi << 0x8 | lo
}

fn step_rng(seed: u32) -> u32 {
    let seed = std::num::Wrapping(seed);
    (((seed * std::num::Wrapping(2)) - (seed >> 0x1f) + std::num::Wrapping(1))
        ^ std::num::Wrapping(0x873ca9e5))
    .0
}

impl hooks::Hooks for BN6 {
    fn common_traps(&self) -> Vec<(u32, Box<dyn FnMut(mgba::core::CoreMutRef)>)> {
        vec![
            {
                let munger = self.munger.clone();
                (
                    self.offsets.rom.start_screen_jump_table_entry,
                    Box::new(move |core| {
                        munger.skip_logo(core);
                    }),
                )
            },
            {
                let munger = self.munger.clone();
                (
                    self.offsets.rom.start_screen_sram_unmask_ret,
                    Box::new(move |core| {
                        munger.continue_from_title_menu(core);
                    }),
                )
            },
            {
                let munger = self.munger.clone();
                (
                    self.offsets.rom.game_load_ret,
                    Box::new(move |core| {
                        munger.open_comm_menu_from_overworld(core);
                    }),
                )
            },
        ]
    }

    fn primary_traps(
        &self,
        handle: tokio::runtime::Handle,
        joyflags: std::sync::Arc<std::sync::atomic::AtomicU32>,
        facade: facade::Facade,
    ) -> Vec<(u32, Box<dyn FnMut(mgba::core::CoreMutRef)>)> {
        vec![
            {
                let facade = facade.clone();
                let handle = handle.clone();
                let munger = self.munger.clone();
                (
                    self.offsets.rom.comm_menu_init_ret,
                    Box::new(move |core| {
                        handle.block_on(async {
                            let match_ = match facade.match_().await {
                                Some(match_) => match_,
                                None => {
                                    return;
                                }
                            };

                            munger.start_battle_from_comm_menu(core, match_.match_type());

                            let mut rng = match_.lock_rng().await;

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
                            munger.set_rng3_state(core, rng2_state);
                        });
                    }),
                )
            },
            {
                let facade = facade.clone();
                let handle = handle.clone();
                (
                    self.offsets.rom.round_end_set_win,
                    Box::new(move |_| {
                        handle.block_on(async {
                            let match_ = match facade.match_().await {
                                Some(match_) => match_,
                                None => {
                                    return;
                                }
                            };

                            let mut round_state = match_.lock_round_state().await;
                            round_state.set_last_result(battle::BattleResult::Win);
                        });
                    }),
                )
            },
            {
                let facade = facade.clone();
                let handle = handle.clone();
                (
                    self.offsets.rom.round_end_set_loss,
                    Box::new(move |_| {
                        handle.block_on(async {
                            let match_ = match facade.match_().await {
                                Some(match_) => match_,
                                None => {
                                    return;
                                }
                            };

                            let mut round_state = match_.lock_round_state().await;
                            round_state.set_last_result(battle::BattleResult::Loss);
                        });
                    }),
                )
            },
            {
                let facade = facade.clone();
                let handle = handle.clone();
                (
                    self.offsets.rom.round_end_damage_judge_set_win,
                    Box::new(move |_| {
                        handle.block_on(async {
                            let match_ = match facade.match_().await {
                                Some(match_) => match_,
                                None => {
                                    return;
                                }
                            };

                            let mut round_state = match_.lock_round_state().await;
                            round_state.set_last_result(battle::BattleResult::Win);
                        });
                    }),
                )
            },
            {
                let facade = facade.clone();
                let handle = handle.clone();
                (
                    self.offsets.rom.round_end_damage_judge_set_loss,
                    Box::new(move |_| {
                        handle.block_on(async {
                            let match_ = match facade.match_().await {
                                Some(match_) => match_,
                                None => {
                                    return;
                                }
                            };

                            let mut round_state = match_.lock_round_state().await;
                            round_state.set_last_result(battle::BattleResult::Loss);
                        });
                    }),
                )
            },
            {
                let facade = facade.clone();
                let handle = handle.clone();
                (
                    self.offsets.rom.round_end_damage_judge_set_draw,
                    Box::new(move |_| {
                        handle.block_on(async {
                            let match_ = match facade.match_().await {
                                Some(match_) => match_,
                                None => {
                                    return;
                                }
                            };

                            let mut round_state = match_.lock_round_state().await;
                            let result = {
                                let round = round_state.round.as_ref().expect("round");
                                round.on_draw_result()
                            };
                            round_state.set_last_result(result);
                        });
                    }),
                )
            },
            {
                let facade = facade.clone();
                let handle = handle.clone();
                (
                    self.offsets.rom.round_set_ending,
                    Box::new(move |_| {
                        handle.block_on(async {
                            let match_ = match facade.match_().await {
                                Some(match_) => match_,
                                None => {
                                    return;
                                }
                            };

                            let mut round_state = match_.lock_round_state().await;
                            round_state.end_round().await.expect("end round");
                            match_
                                .advance_shadow_until_round_end()
                                .await
                                .expect("advance shadow");
                        });
                    }),
                )
            },
            {
                let facade = facade.clone();
                let handle = handle.clone();
                (
                    self.offsets.rom.round_start_ret,
                    Box::new(move |_core| {
                        handle.block_on(async {
                            let match_ = match facade.match_().await {
                                Some(match_) => match_,
                                None => {
                                    return;
                                }
                            };
                            match_.start_round().await.expect("start round");
                        });
                    }),
                )
            },
            {
                let facade = facade.clone();
                let handle = handle.clone();
                (
                    self.offsets.rom.battle_is_p2_tst,
                    Box::new(move |mut core| {
                        handle.block_on(async {
                            let match_ = match facade.match_().await {
                                Some(match_) => match_,
                                None => {
                                    return;
                                }
                            };

                            let round_state = match_.lock_round_state().await;
                            let round = round_state.round.as_ref().expect("round");

                            core.gba_mut()
                                .cpu_mut()
                                .set_gpr(0, round.local_player_index() as i32);
                        });
                    }),
                )
            },
            {
                let facade = facade.clone();
                let handle = handle.clone();
                (
                    self.offsets.rom.link_is_p2_ret,
                    Box::new(move |mut core| {
                        handle.block_on(async {
                            let match_ = match facade.match_().await {
                                Some(match_) => match_,
                                None => {
                                    return;
                                }
                            };

                            let round_state = match_.lock_round_state().await;
                            let round = round_state.round.as_ref().expect("round");

                            core.gba_mut()
                                .cpu_mut()
                                .set_gpr(0, round.local_player_index() as i32);
                        });
                    }),
                )
            },
            {
                (
                    self.offsets.rom.handle_sio_entry,
                    Box::new(move |core| {
                        log::error!(
                            "unhandled call to handleSIO at 0x{:0x}: uh oh!",
                            core.as_ref().gba().cpu().gpr(14) - 2
                        );
                    }),
                )
            },
            {
                let facade = facade.clone();
                let munger = self.munger.clone();
                let handle = handle.clone();
                (
                    self.offsets.rom.comm_menu_init_battle_entry,
                    Box::new(move |core| {
                        handle.block_on(async {
                            let match_ = match facade.match_().await {
                                Some(match_) => match_,
                                None => {
                                    return;
                                }
                            };

                            let mut rng = match_.lock_rng().await;
                            munger.set_link_battle_settings_and_background(
                                core,
                                random_battle_settings_and_background(
                                    &mut *rng,
                                    match_.match_type(),
                                ),
                            );
                        });
                    }),
                )
            },
            {
                let facade = facade.clone();
                let handle = handle.clone();
                (
                    self.offsets.rom.comm_menu_end_battle_entry,
                    Box::new(move |_core| {
                        handle.block_on(async {
                            log::info!("match ended");
                            facade.end_match().await;
                        });
                    }),
                )
            },
            {
                let facade = facade.clone();
                let munger = self.munger.clone();
                let handle = handle.clone();
                (
                    self.offsets
                        .rom
                        .comm_menu_in_battle_call_comm_menu_handle_link_cable_input,
                    Box::new(move |mut core| {
                        handle.block_on(async {
                            let pc = core.as_ref().gba().cpu().thumb_pc() as u32;
                            core.gba_mut().cpu_mut().set_thumb_pc(pc + 6);
                            munger.set_copy_data_input_state(
                                core,
                                if facade.match_().await.is_some() {
                                    2
                                } else {
                                    4
                                },
                            );
                        });
                    }),
                )
            },
            {
                let facade = facade.clone();
                let munger = self.munger.clone();
                let handle = handle.clone();
                (
                    self.offsets.rom.main_read_joyflags,
                    Box::new(move |core| {
                        handle.block_on(async {
                            'abort: loop {
                                let match_ = match facade.match_().await {
                                    Some(match_) => match_,
                                    None => {
                                        return;
                                    }
                                };

                                let mut round_state = match_.lock_round_state().await;

                                let round = match round_state.round.as_mut() {
                                    Some(round) => round,
                                    None => {
                                        return;
                                    }
                                };

                                if !round.has_committed_state() {
                                    // HACK: The battle jump table goes directly from deinit to init, so we actually end up initializing on tick 1 after round 1. We just override it here.
                                    munger.set_current_tick(core, 0);

                                    round.set_first_committed_state(
                                        core.save_state().expect("save state"),
                                        match_
                                            .advance_shadow_until_first_committed_state()
                                            .await
                                            .expect("shadow save state"),
                                        &munger.tx_packet(core),
                                    );

                                    log::info!(
                                        "primary rng1 state: {:08x}, rng2 state: {:08x}, rng3 state: {:08x}",
                                        munger.rng1_state(core),
                                        munger.rng2_state(core),
                                        munger.rng3_state(core),
                                    );
                                    log::info!(
                                        "battle state committed on {}",
                                        round.current_tick()
                                    );
                                }

                                let game_current_tick = munger.current_tick(core);
                                if game_current_tick != round.current_tick() {
                                    panic!(
                                        "read joyflags: round tick = {} but game tick = {}",
                                        round.current_tick(),
                                        game_current_tick
                                    );
                                }

                                if let Err(e) = round
                                    .add_local_input_and_fastforward(
                                        core,
                                        joyflags.load(std::sync::atomic::Ordering::Relaxed) as u16,
                                    )
                                    .await
                                {
                                    log::error!("failed to add local input: {}", e);
                                    break 'abort;
                                }
                                return;
                            }
                            facade.abort_match().await;
                        });
                    }),
                )
            },
            {
                let facade = facade.clone();
                let munger = self.munger.clone();
                let handle = handle.clone();
                (
                    self.offsets.rom.round_post_increment_tick,
                    Box::new(move |core| {
                        handle.block_on(async {
                            let match_ = match facade.match_().await {
                                Some(match_) => match_,
                                None => {
                                    return;
                                }
                            };

                            let mut round_state = match_.lock_round_state().await;

                            let round = match round_state.round.as_mut() {
                                Some(round) => round,
                                None => {
                                    return;
                                }
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
                        });
                    }),
                )
            },
        ]
    }

    fn shadow_traps(
        &self,
        shadow_state: shadow::State,
    ) -> Vec<(u32, Box<dyn FnMut(mgba::core::CoreMutRef)>)> {
        vec![
            {
                let munger = self.munger.clone();
                let shadow_state = shadow_state.clone();
                (
                    self.offsets.rom.comm_menu_init_ret,
                    Box::new(move |core| {
                        munger.start_battle_from_comm_menu(core, shadow_state.match_type());

                        let mut rng = shadow_state.lock_rng();

                        // rng1 is the local rng, it should not be synced.
                        // However, we should make sure it's reproducible from the shared RNG state so we generate it like this.
                        let offerer_rng1_state = generate_rng1_state(&mut *rng);
                        let answerer_rng1_state = generate_rng1_state(&mut *rng);
                        munger.set_rng1_state(
                            core,
                            if shadow_state.is_offerer() {
                                answerer_rng1_state
                            } else {
                                offerer_rng1_state
                            },
                        );

                        // rng2 is the shared rng, it must be synced.
                        let rng2_state = generate_rng2_state(&mut *rng);
                        munger.set_rng2_state(core, rng2_state);
                        munger.set_rng3_state(core, rng2_state);
                    }),
                )
            },
            {
                let shadow_state = shadow_state.clone();
                (
                    self.offsets.rom.round_end_set_win,
                    Box::new(move |_| {
                        let mut round_state = shadow_state.lock_round_state();
                        round_state.set_last_result(battle::BattleResult::Loss);
                    }),
                )
            },
            {
                let shadow_state = shadow_state.clone();
                (
                    self.offsets.rom.round_end_set_loss,
                    Box::new(move |_| {
                        let mut round_state = shadow_state.lock_round_state();
                        round_state.set_last_result(battle::BattleResult::Win);
                    }),
                )
            },
            {
                let shadow_state = shadow_state.clone();
                (
                    self.offsets.rom.round_end_damage_judge_set_win,
                    Box::new(move |_| {
                        let mut round_state = shadow_state.lock_round_state();
                        round_state.set_last_result(battle::BattleResult::Loss);
                    }),
                )
            },
            {
                let shadow_state = shadow_state.clone();
                (
                    self.offsets.rom.round_end_damage_judge_set_loss,
                    Box::new(move |_| {
                        let mut round_state = shadow_state.lock_round_state();
                        round_state.set_last_result(battle::BattleResult::Win);
                    }),
                )
            },
            {
                let shadow_state = shadow_state.clone();
                (
                    self.offsets.rom.round_end_damage_judge_set_draw,
                    Box::new(move |_| {
                        let mut round_state = shadow_state.lock_round_state();
                        let result = {
                            let round = round_state.round.as_mut().expect("round");
                            round.on_draw_result()
                        };
                        round_state.set_last_result(result);
                    }),
                )
            },
            {
                let shadow_state = shadow_state.clone();
                (
                    self.offsets.rom.round_start_ret,
                    Box::new(move |_| {
                        shadow_state.start_round();
                    }),
                )
            },
            {
                let shadow_state = shadow_state.clone();
                (
                    self.offsets.rom.round_end_entry,
                    Box::new(move |core| {
                        shadow_state.end_round();
                        shadow_state.set_applied_state(core.save_state().expect("save state"), 0);
                    }),
                )
            },
            {
                let shadow_state = shadow_state.clone();
                (
                    self.offsets.rom.battle_is_p2_tst,
                    Box::new(move |mut core| {
                        let mut round_state = shadow_state.lock_round_state();
                        let round = round_state.round.as_mut().expect("round");

                        core.gba_mut()
                            .cpu_mut()
                            .set_gpr(0, round.remote_player_index() as i32);
                    }),
                )
            },
            {
                let shadow_state = shadow_state.clone();
                (
                    self.offsets.rom.link_is_p2_ret,
                    Box::new(move |mut core| {
                        let mut round_state = shadow_state.lock_round_state();
                        let round = round_state.round.as_mut().expect("round");

                        core.gba_mut()
                            .cpu_mut()
                            .set_gpr(0, round.remote_player_index() as i32);
                    }),
                )
            },
            {
                (
                    self.offsets.rom.handle_sio_entry,
                    Box::new(move |core| {
                        log::error!(
                            "unhandled call to handleSIO at 0x{:0x}: uh oh!",
                            core.as_ref().gba().cpu().gpr(14) - 2
                        );
                    }),
                )
            },
            {
                let shadow_state = shadow_state.clone();
                let munger = self.munger.clone();
                (
                    self.offsets.rom.comm_menu_init_battle_entry,
                    Box::new(move |core| {
                        let mut rng = shadow_state.lock_rng();
                        munger.set_link_battle_settings_and_background(
                            core,
                            random_battle_settings_and_background(
                                &mut *rng,
                                shadow_state.match_type(),
                            ),
                        );
                    }),
                )
            },
            {
                let munger = self.munger.clone();
                (
                    self.offsets
                        .rom
                        .comm_menu_in_battle_call_comm_menu_handle_link_cable_input,
                    Box::new(move |mut core| {
                        let pc = core.as_ref().gba().cpu().thumb_pc() as u32;
                        core.gba_mut().cpu_mut().set_thumb_pc(pc + 6);
                        munger.set_copy_data_input_state(core, 2);
                    }),
                )
            },
            {
                let shadow_state = shadow_state.clone();
                let munger = self.munger.clone();
                (
                    self.offsets.rom.main_read_joyflags,
                    Box::new(move |mut core| {
                        let mut round_state = shadow_state.lock_round_state();
                        let round = match round_state.round.as_mut() {
                            Some(round) => round,
                            None => {
                                return;
                            }
                        };

                        if !round.has_first_committed_state() {
                            // HACK: The battle jump table goes directly from deinit to init, so we actually end up initializing on tick 1 after round 1. We just override it here.
                            munger.set_current_tick(core, 0);

                            round.set_first_committed_state(
                                core.save_state().expect("save state"),
                                &munger.tx_packet(core),
                            );
                            log::info!(
                                "shadow rng1 state: {:08x}, rng2 state: {:08x}, rng3 state: {:08x}",
                                munger.rng1_state(core),
                                munger.rng2_state(core),
                                munger.rng3_state(core)
                            );
                            log::info!("shadow state committed on {}", round.current_tick());
                            return;
                        }

                        let game_current_tick = munger.current_tick(core);
                        if game_current_tick != round.current_tick() {
                            shadow_state.set_anyhow_error(anyhow::anyhow!(
                                "read joyflags: round tick = {} but game tick = {}",
                                round.current_tick(),
                                game_current_tick
                            ));
                        }

                        if let Some(ip) = round.peek_shadow_input().clone() {
                            if ip.local.local_tick != ip.remote.local_tick {
                                shadow_state.set_anyhow_error(anyhow::anyhow!(
                                    "read joyflags: local tick != remote tick (in battle tick = {}): {} != {}",
                                    round.current_tick(),
                                    ip.local.local_tick,
                                    ip.remote.local_tick
                                ));
                                return;
                            }

                            if ip.local.local_tick != round.current_tick() {
                                shadow_state.set_anyhow_error(anyhow::anyhow!(
                                    "read joyflags: input tick != in battle tick: {} != {}",
                                    ip.local.local_tick,
                                    round.current_tick(),
                                ));
                                return;
                            }

                            core.gba_mut()
                                .cpu_mut()
                                .set_gpr(4, (ip.remote.joyflags | 0xfc00) as i32);
                        }

                        if round.take_input_injected() {
                            shadow_state.set_applied_state(
                                core.save_state().expect("save state"),
                                round.current_tick(),
                            );
                        }
                    }),
                )
            },
            {
                let shadow_state = shadow_state.clone();
                let munger = self.munger.clone();
                (
                    self.offsets.rom.copy_input_data_entry,
                    Box::new(move |core| {
                        let mut round_state = shadow_state.lock_round_state();
                        let round = round_state.round.as_mut().expect("round");

                        let game_current_tick = munger.current_tick(core);
                        if game_current_tick != round.current_tick() {
                            shadow_state.set_anyhow_error(anyhow::anyhow!(
                                "copy input data: round tick = {} but game tick = {}",
                                round.current_tick(),
                                game_current_tick
                            ));
                        }

                        let ip = if let Some(ip) = round.take_shadow_input() {
                            ip
                        } else {
                            return;
                        };

                        // HACK: This is required if the emulator advances beyond read joyflags and runs this function again, but is missing input data.
                        // We permit this for one tick only, but really we should just not be able to get into this situation in the first place.
                        if ip.local.local_tick + 1 == round.current_tick() {
                            return;
                        }

                        if ip.local.local_tick != ip.remote.local_tick {
                            shadow_state.set_anyhow_error(anyhow::anyhow!(
                                "copy input data: local tick != remote tick (in battle tick = {}): {} != {}",
                                round.current_tick(),
                                ip.local.local_tick,
                                ip.remote.local_tick
                            ));
                            return;
                        }

                        if ip.local.local_tick != round.current_tick() {
                            shadow_state.set_anyhow_error(anyhow::anyhow!(
                                "copy input data: input tick != in battle tick: {} != {}",
                                ip.local.local_tick,
                                round.current_tick(),
                            ));
                            return;
                        }

                        let remote_packet = round.peek_remote_packet().unwrap();
                        if remote_packet.tick != round.current_tick() {
                            shadow_state.set_anyhow_error(anyhow::anyhow!(
                                "copy input data: local packet tick != in battle tick: {} != {}",
                                remote_packet.tick,
                                round.current_tick(),
                            ));
                            return;
                        }

                        munger.set_rx_packet(
                            core,
                            round.local_player_index() as u32,
                            &ip.local.packet.try_into().unwrap(),
                        );
                        munger.set_rx_packet(
                            core,
                            round.remote_player_index() as u32,
                            &remote_packet.packet.clone().try_into().unwrap(),
                        );
                    }),
                )
            },
            {
                let shadow_state = shadow_state.clone();
                let munger = self.munger.clone();
                (
                    self.offsets.rom.copy_input_data_ret,
                    Box::new(move |core| {
                        let mut round_state = shadow_state.lock_round_state();
                        let round = round_state.round.as_mut().expect("round");

                        let game_current_tick = munger.current_tick(core);
                        if game_current_tick != round.current_tick() {
                            shadow_state.set_anyhow_error(anyhow::anyhow!(
                                "copy input data: round tick = {} but game tick = {}",
                                round.current_tick(),
                                game_current_tick
                            ));
                        }

                        round.set_remote_packet(
                            round.current_tick() + 1,
                            munger.tx_packet(core).to_vec(),
                        );
                        round.set_input_injected();
                    }),
                )
            },
            {
                let shadow_state = shadow_state.clone();
                let munger = self.munger.clone();
                (
                    self.offsets.rom.round_post_increment_tick,
                    Box::new(move |core| {
                        let mut round_state = shadow_state.lock_round_state();
                        let round = round_state.round.as_mut().expect("round");
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
                    }),
                )
            },
        ]
    }

    fn replayer_traps(
        &self,
        replayer_state: replayer::State,
    ) -> Vec<(u32, Box<dyn FnMut(mgba::core::CoreMutRef)>)> {
        vec![
            {
                let replayer_state = replayer_state.clone();
                (
                    self.offsets.rom.battle_is_p2_tst,
                    Box::new(move |mut core| {
                        let replayer_state = replayer_state.lock_inner();
                        core.gba_mut()
                            .cpu_mut()
                            .set_gpr(0, replayer_state.local_player_index() as i32);
                    }),
                )
            },
            {
                let replayer_state = replayer_state.clone();
                (
                    self.offsets.rom.link_is_p2_ret,
                    Box::new(move |mut core| {
                        let replayer_state = replayer_state.lock_inner();
                        core.gba_mut()
                            .cpu_mut()
                            .set_gpr(0, replayer_state.local_player_index() as i32);
                    }),
                )
            },
            {
                let munger = self.munger.clone();
                (
                    self.offsets
                        .rom
                        .comm_menu_in_battle_call_comm_menu_handle_link_cable_input,
                    Box::new(move |mut core| {
                        let pc = core.as_ref().gba().cpu().thumb_pc() as u32;
                        core.gba_mut().cpu_mut().set_thumb_pc(pc + 6);
                        munger.set_copy_data_input_state(core, 2);
                    }),
                )
            },
            {
                let replayer_state = replayer_state.clone();
                (
                    self.offsets.rom.round_set_ending,
                    Box::new(move |_core| {
                        let mut replayer_state = replayer_state.lock_inner();
                        replayer_state.set_round_ending();
                    }),
                )
            },
            {
                let replayer_state = replayer_state.clone();
                (
                    self.offsets.rom.round_end_entry,
                    Box::new(move |_core| {
                        let mut replayer_state = replayer_state.lock_inner();
                        replayer_state.set_round_ended();
                    }),
                )
            },
            {
                let munger = self.munger.clone();
                let replayer_state = replayer_state.clone();
                (
                    self.offsets.rom.main_read_joyflags,
                    Box::new(move |mut core| {
                        let mut replayer_state = replayer_state.lock_inner();
                        let current_tick = replayer_state.current_tick();

                        let game_current_tick = munger.current_tick(core);
                        if game_current_tick != current_tick {
                            panic!(
                                "round tick = {} but game tick = {}",
                                current_tick, game_current_tick
                            );
                        }

                        if current_tick == replayer_state.commit_tick() {
                            replayer_state.set_committed_state(
                                core.save_state().expect("save committed state"),
                            );
                        }

                        let ip = match replayer_state.peek_input_pair() {
                            Some(ip) => ip.clone(),
                            None => {
                                return;
                            }
                        };

                        if ip.local.local_tick != ip.remote.local_tick {
                            replayer_state.set_anyhow_error(anyhow::anyhow!(
                                "read joyflags: local tick != remote tick (in battle tick = {}): {} != {}",
                                current_tick,
                                ip.local.local_tick,
                                ip.remote.local_tick
                            ));
                            return;
                        }

                        if ip.local.local_tick != current_tick {
                            replayer_state.set_anyhow_error(anyhow::anyhow!(
                                "read joyflags: input tick != in battle tick: {} != {}",
                                ip.local.local_tick,
                                current_tick,
                            ));
                            return;
                        }

                        core.gba_mut()
                            .cpu_mut()
                            .set_gpr(4, (ip.local.joyflags | 0xfc00) as i32);

                        if current_tick == replayer_state.dirty_tick() {
                            replayer_state
                                .set_dirty_state(core.save_state().expect("save dirty state"));
                        }
                    }),
                )
            },
            {
                let munger = self.munger.clone();
                let replayer_state = replayer_state.clone();
                (
                    self.offsets.rom.copy_input_data_entry,
                    Box::new(move |core| {
                        let mut replayer_state = replayer_state.lock_inner();
                        if replayer_state.is_round_ending() {
                            return;
                        }

                        let current_tick = replayer_state.current_tick();

                        let game_current_tick = munger.current_tick(core);
                        if game_current_tick != current_tick {
                            panic!(
                                "round tick = {} but game tick = {}",
                                current_tick, game_current_tick
                            );
                        }

                        let ip = match replayer_state.pop_input_pair() {
                            Some(ip) => ip.clone(),
                            None => {
                                return;
                            }
                        };

                        if ip.local.local_tick != ip.remote.local_tick {
                            replayer_state.set_anyhow_error(anyhow::anyhow!(
                                "copy input data: local tick != remote tick (in battle tick = {}): {} != {}",
                                current_tick,
                                ip.local.local_tick,
                                ip.remote.local_tick
                            ));
                            return;
                        }

                        if ip.local.local_tick != current_tick {
                            replayer_state.set_anyhow_error(anyhow::anyhow!(
                                "copy input data: input tick != in battle tick: {} != {}",
                                ip.local.local_tick,
                                current_tick,
                            ));
                            return;
                        }

                        let local_packet = replayer_state.peek_local_packet().unwrap().clone();
                        if local_packet.tick != current_tick {
                            replayer_state.set_anyhow_error(anyhow::anyhow!(
                                "copy input data: local packet tick != in battle tick: {} != {}",
                                local_packet.tick,
                                current_tick,
                            ));
                            return;
                        }

                        munger.set_rx_packet(
                            core,
                            replayer_state.local_player_index() as u32,
                            &local_packet.packet.clone().try_into().unwrap(),
                        );
                        munger.set_rx_packet(
                            core,
                            replayer_state.remote_player_index() as u32,
                            &replayer_state
                                .apply_shadow_input(input::Pair {
                                    local: ip.local.with_packet(local_packet.packet),
                                    remote: ip.remote,
                                })
                                .expect("apply shadow input")
                                .try_into()
                                .unwrap(),
                        );
                    }),
                )
            },
            {
                let munger = self.munger.clone();
                let replayer_state = replayer_state.clone();
                (
                    self.offsets.rom.copy_input_data_ret,
                    Box::new(move |core| {
                        let mut replayer_state = replayer_state.lock_inner();
                        if replayer_state.is_round_ending() {
                            return;
                        }

                        let current_tick = replayer_state.current_tick();

                        let game_current_tick = munger.current_tick(core);
                        if game_current_tick != current_tick {
                            panic!(
                                "round tick = {} but game tick = {}",
                                current_tick, game_current_tick
                            );
                        }

                        replayer_state
                            .set_local_packet(current_tick + 1, munger.tx_packet(core).to_vec());
                    }),
                )
            },
            {
                let replayer_state = replayer_state.clone();
                let munger = self.munger.clone();
                (
                    self.offsets.rom.round_post_increment_tick,
                    Box::new(move |core| {
                        let mut replayer_state = replayer_state.lock_inner();
                        replayer_state.increment_current_tick();
                        let current_tick = replayer_state.current_tick();

                        let game_current_tick = munger.current_tick(core);
                        if game_current_tick != current_tick {
                            replayer_state.set_anyhow_error(anyhow::anyhow!(
                                "post increment tick: round tick = {} but game tick = {}",
                                current_tick,
                                game_current_tick
                            ));
                        }
                    }),
                )
            },
            {
                let replayer_state = replayer_state.clone();
                (
                    self.offsets.rom.round_end_set_win,
                    Box::new(move |_| {
                        let mut replayer_state = replayer_state.lock_inner();
                        replayer_state.set_round_result(replayer::BattleResult::Win);
                    }),
                )
            },
            {
                let replayer_state = replayer_state.clone();
                (
                    self.offsets.rom.round_end_set_loss,
                    Box::new(move |_| {
                        let mut replayer_state = replayer_state.lock_inner();
                        replayer_state.set_round_result(replayer::BattleResult::Loss);
                    }),
                )
            },
            {
                let replayer_state = replayer_state.clone();
                (
                    self.offsets.rom.round_end_damage_judge_set_win,
                    Box::new(move |_| {
                        let mut replayer_state = replayer_state.lock_inner();
                        replayer_state.set_round_result(replayer::BattleResult::Win);
                    }),
                )
            },
            {
                let replayer_state = replayer_state.clone();
                (
                    self.offsets.rom.round_end_damage_judge_set_loss,
                    Box::new(move |_| {
                        let mut replayer_state = replayer_state.lock_inner();
                        replayer_state.set_round_result(replayer::BattleResult::Loss);
                    }),
                )
            },
            {
                let replayer_state = replayer_state.clone();
                (
                    self.offsets.rom.round_end_damage_judge_set_draw,
                    Box::new(move |_| {
                        let mut replayer_state = replayer_state.lock_inner();
                        replayer_state.set_round_result(replayer::BattleResult::Draw);
                    }),
                )
            },
        ]
    }

    fn prepare_for_fastforward(&self, mut core: mgba::core::CoreMutRef) {
        core.gba_mut()
            .cpu_mut()
            .set_thumb_pc(self.offsets.rom.main_read_joyflags);
    }

    fn replace_opponent_name(&self, mut core: mgba::core::CoreMutRef, name: &str) {
        if self.offsets.rom.opponent_name == 0 {
            // Not whimsical enough :(
            return;
        }
        if name.is_empty() {
            return;
        }
        const CHARS: &str = " 0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ*abcdefghijklmnopqrstuvwxyz";
        const MAX_NAME_LEN: usize = 9;
        const EOS: u8 = 0xe6;
        let mut buf = Vec::with_capacity(MAX_NAME_LEN);
        for c in name.chars() {
            if buf.len() == MAX_NAME_LEN {
                break;
            }

            buf.push(if let Some(i) = CHARS.find(c) {
                i as u8
            } else {
                0
            });
        }
        buf.push(EOS);
        core.raw_write_range(self.offsets.rom.opponent_name, -1, &buf);
    }
}
