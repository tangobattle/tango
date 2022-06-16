use crate::{battle, facade, hooks, input, replayer, shadow};

mod munger;
mod offsets;

#[derive(Clone)]
pub struct BN5 {
    offsets: offsets::Offsets,
    munger: munger::Munger,
}

lazy_static! {
    pub static ref MEGAMAN5_TP_BRBE_00: Box<dyn hooks::Hooks + Send + Sync> =
        BN5::new(offsets::MEGAMAN5_TP_BRBE_00);
    pub static ref MEGAMAN5_TC_BRKE_00: Box<dyn hooks::Hooks + Send + Sync> =
        BN5::new(offsets::MEGAMAN5_TC_BRKE_00);
    pub static ref ROCKEXE5_TOBBRBJ_00: Box<dyn hooks::Hooks + Send + Sync> =
        BN5::new(offsets::ROCKEXE5_TOBBRBJ_00);
    pub static ref ROCKEXE5_TOCBRKJ_00: Box<dyn hooks::Hooks + Send + Sync> =
        BN5::new(offsets::ROCKEXE5_TOCBRKJ_00);
}

impl BN5 {
    pub fn new(offsets: offsets::Offsets) -> Box<dyn hooks::Hooks + Send + Sync> {
        Box::new(BN5 {
            offsets,
            munger: munger::Munger { offsets },
        })
    }
}

fn generate_rng1_state(rng: &mut impl rand::Rng) -> u32 {
    let mut rng1_state = 0;
    for _ in 0..rng.gen_range(0..=0xffffusize) {
        rng1_state = step_rng(rng1_state);
    }
    rng1_state
}

fn generate_rng2_state(rng: &mut impl rand::Rng) -> u32 {
    let mut rng2_state = 0xa338244f;
    for _ in 0..rng.gen_range(0..=0xffffusize) {
        rng2_state = step_rng(rng2_state);
    }
    rng2_state
}

fn random_battle_settings_and_background(rng: &mut impl rand::Rng) -> (u8, u8) {
    // BN5 has 0x60 stages, but the remaining ones are only used in "battle" mode.
    (rng.gen_range(0..0x44u8), rng.gen_range(0..0x1bu8))
}

fn step_rng(seed: u32) -> u32 {
    let seed = std::num::Wrapping(seed);
    (((seed * std::num::Wrapping(2)) - (seed >> 0x1f) + std::num::Wrapping(1))
        ^ std::num::Wrapping(0x873ca9e5))
    .0
}

impl hooks::Hooks for BN5 {
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
                            munger.set_rng2_state(core, generate_rng2_state(&mut *rng));
                        });
                    }),
                )
            },
            {
                let facade = facade.clone();
                let handle = handle.clone();
                (
                    self.offsets.rom.round_run_unpaused_step_cmp_retval,
                    Box::new(move |core| {
                        handle.block_on(async {
                            let match_ = match facade.match_().await {
                                Some(match_) => match_,
                                None => {
                                    return;
                                }
                            };

                            let mut round_state = match_.lock_round_state().await;

                            match core.as_ref().gba().cpu().gpr(0) {
                                1 => {
                                    round_state.set_last_result(battle::BattleResult::Loss);
                                }
                                2 => {
                                    round_state.set_last_result(battle::BattleResult::Win);
                                }
                                7 => {
                                    round_state.set_last_result(battle::BattleResult::Draw);
                                }
                                _ => {}
                            }
                        });
                    }),
                )
            },
            {
                let facade = facade.clone();
                let handle = handle.clone();
                (
                    self.offsets.rom.round_ending_ret,
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
                            let (battle_settings, background) =
                                random_battle_settings_and_background(&mut *rng);
                            munger.set_battle_settings_and_background(
                                core,
                                battle_settings,
                                background,
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
                    self.offsets.rom.in_battle_call_handle_link_cable_input,
                    Box::new(move |mut core| {
                        handle.block_on(async {
                            let pc = core.as_ref().gba().cpu().thumb_pc() as u32;
                            core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
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
                                    );
                                    log::info!(
                                        "primary rng1 state: {:08x}",
                                        munger.rng1_state(core)
                                    );
                                    log::info!(
                                        "primary rng2 state: {:08x}",
                                        munger.rng2_state(core)
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

                                if !round
                                    .add_local_input_and_fastforward(
                                        core,
                                        joyflags.load(std::sync::atomic::Ordering::Relaxed) as u16,
                                    )
                                    .await
                                {
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
                    self.offsets.rom.copy_input_data_ret,
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

                            let current_tick = munger.current_tick(core);
                            if current_tick != round.current_tick() {
                                panic!(
                                    "primary: round tick = {} but game tick = {}",
                                    round.current_tick(),
                                    current_tick
                                );
                            }

                            round.queue_tx(
                                round.current_tick() + 1,
                                munger.tx_packet(core).to_vec(),
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
                        munger.set_rng2_state(core, generate_rng2_state(&mut *rng));
                    }),
                )
            },
            {
                let shadow_state = shadow_state.clone();
                (
                    self.offsets.rom.round_run_unpaused_step_cmp_retval,
                    Box::new(move |core| {
                        match core.as_ref().gba().cpu().gpr(0) {
                            1 => {
                                shadow_state.set_last_result(battle::BattleResult::Win);
                            }
                            2 => {
                                shadow_state.set_last_result(battle::BattleResult::Loss);
                            }
                            7 => {
                                shadow_state.set_last_result(battle::BattleResult::Draw);
                            }
                            _ => {}
                        };
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
                        let (battle_settings, background) =
                            random_battle_settings_and_background(&mut *rng);
                        munger.set_battle_settings_and_background(
                            core,
                            battle_settings,
                            background,
                        );
                    }),
                )
            },
            {
                let munger = self.munger.clone();
                (
                    self.offsets.rom.in_battle_call_handle_link_cable_input,
                    Box::new(move |mut core| {
                        let pc = core.as_ref().gba().cpu().thumb_pc() as u32;
                        core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
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

                            round.set_first_committed_state(core.save_state().expect("save state"));
                            log::info!("shadow rng1 state: {:08x}", munger.rng1_state(core));
                            log::info!("shadow rng2 state: {:08x}", munger.rng2_state(core));
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

                        if let Some(ip) = round.take_in_input_pair() {
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

                            round.set_out_input_pair(input::Pair {
                                local: ip.local,
                                remote: input::Input {
                                    local_tick: ip.remote.local_tick,
                                    remote_tick: ip.remote.remote_tick,
                                    joyflags: ip.remote.joyflags,
                                    rx: munger.tx_packet(core).to_vec(),
                                    is_prediction: false,
                                },
                            });

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

                        let ip = if let Some(ip) = round.peek_out_input_pair().as_ref() {
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

                        munger.set_rx_packet(
                            core,
                            round.local_player_index() as u32,
                            &ip.local.rx.clone().try_into().unwrap(),
                        );

                        munger.set_rx_packet(
                            core,
                            round.remote_player_index() as u32,
                            &ip.remote.rx.clone().try_into().unwrap(),
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
                        core.gba_mut()
                            .cpu_mut()
                            .set_gpr(0, replayer_state.local_player_index() as i32);
                    }),
                )
            },
            {
                let munger = self.munger.clone();
                (
                    self.offsets.rom.in_battle_call_handle_link_cable_input,
                    Box::new(move |mut core| {
                        let pc = core.as_ref().gba().cpu().thumb_pc() as u32;
                        core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
                        munger.set_copy_data_input_state(core, 2);
                    }),
                )
            },
            {
                let replayer_state = replayer_state.clone();
                (
                    self.offsets.rom.round_end_entry,
                    Box::new(move |_core| {
                        replayer_state.on_round_ended();
                    }),
                )
            },
            {
                let munger = self.munger.clone();
                let replayer_state = replayer_state.clone();
                (
                    self.offsets.rom.main_read_joyflags,
                    Box::new(move |mut core| {
                        let current_tick = replayer_state.current_tick();

                        let game_current_tick = munger.current_tick(core);
                        if game_current_tick != current_tick {
                            panic!(
                                "round tick = {} but game tick = {}",
                                current_tick, game_current_tick
                            );
                        }

                        if current_tick == replayer_state.commit_time() {
                            replayer_state.set_committed_state(
                                core.save_state().expect("save committed state"),
                            );
                        }

                        let ip = match replayer_state.peek_input_pair() {
                            Some(ip) => ip,
                            None => {
                                replayer_state.on_inputs_exhausted();
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

                        if current_tick == replayer_state.dirty_time() {
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
                        let current_tick = replayer_state.current_tick();

                        let game_current_tick = munger.current_tick(core);
                        if game_current_tick != current_tick {
                            panic!(
                                "round tick = {} but game tick = {}",
                                current_tick, game_current_tick
                            );
                        }

                        let ip = match replayer_state.pop_input_pair() {
                            Some(ip) => ip,
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

                        munger.set_rx_packet(
                            core,
                            replayer_state.local_player_index() as u32,
                            &ip.local.rx.try_into().unwrap(),
                        );

                        munger.set_rx_packet(
                            core,
                            replayer_state.remote_player_index() as u32,
                            &ip.remote.rx.try_into().unwrap(),
                        );
                    }),
                )
            },
            {
                let replayer_state = replayer_state.clone();
                let munger = self.munger.clone();
                (
                    self.offsets.rom.round_post_increment_tick,
                    Box::new(move |core| {
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
        ]
    }

    fn placeholder_rx(&self) -> Vec<u8> {
        vec![
            0x00, 0x00, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ]
    }

    fn prepare_for_fastforward(&self, mut core: mgba::core::CoreMutRef) {
        core.gba_mut()
            .cpu_mut()
            .set_thumb_pc(self.offsets.rom.main_read_joyflags);
    }
}
