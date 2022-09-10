use crate::{battle, game, lockstep, replayer, session, shadow};

mod munger;
mod offsets;

pub struct Hooks {
    offsets: &'static offsets::Offsets,
}

impl Hooks {
    fn munger(&self) -> munger::Munger {
        munger::Munger { offsets: self.offsets }
    }
}

pub static BRBE_00: Hooks = Hooks {
    offsets: &offsets::BRBE_00,
};
pub static BRKE_00: Hooks = Hooks {
    offsets: &offsets::BRKE_00,
};
pub static BRBJ_00: Hooks = Hooks {
    offsets: &offsets::BRBJ_00,
};
pub static BRKJ_00: Hooks = Hooks {
    offsets: &offsets::BRKJ_00,
};

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

fn random_battle_settings_and_background(extended: bool, rng: &mut impl rand::Rng) -> (u8, u8) {
    (
        rng.gen_range(0..if !extended { 0x44u8 } else { 0x60 }),
        rng.gen_range(0..0x1bu8),
    )
}

fn step_rng(seed: u32) -> u32 {
    let seed = std::num::Wrapping(seed);
    ((seed << 1) + (seed >> 0x1f) + std::num::Wrapping(1)).0 ^ 0x873ca9e5
}

impl game::Hooks for Hooks {
    fn common_traps(&self) -> Vec<(u32, Box<dyn Fn(mgba::core::CoreMutRef)>)> {
        vec![
            (self.offsets.rom.start_screen_jump_table_entry, {
                let munger = self.munger();
                Box::new(move |core| {
                    munger.skip_logo(core);
                })
            }),
            (self.offsets.rom.start_screen_sram_unmask_ret, {
                let munger = self.munger();
                Box::new(move |core| {
                    munger.continue_from_title_menu(core);
                })
            }),
            (self.offsets.rom.game_load_ret, {
                let munger = self.munger();
                Box::new(move |core| {
                    munger.open_comm_menu_from_overworld(core);
                })
            }),
        ]
    }

    fn primary_traps(
        &self,
        joyflags: std::sync::Arc<std::sync::atomic::AtomicU32>,
        match_: std::sync::Arc<tokio::sync::Mutex<Option<std::sync::Arc<battle::Match>>>>,
        completion_token: session::CompletionToken,
    ) -> Vec<(u32, Box<dyn Fn(mgba::core::CoreMutRef)>)> {
        vec![
            (self.offsets.rom.comm_menu_init_ret, {
                let match_ = match_.clone();
                let munger = self.munger();
                Box::new(move |core| {
                    tokio::runtime::Handle::current().block_on(async {
                        let match_ = match_.lock().await;
                        let match_ = match &*match_ {
                            Some(match_) => match_,
                            _ => {
                                return;
                            }
                        };

                        munger.start_battle_from_comm_menu(core, match_.match_type().0);
                    });
                })
            }),
            (self.offsets.rom.round_end_set_win, {
                let match_ = match_.clone();
                Box::new(move |_| {
                    tokio::runtime::Handle::current().block_on(async {
                        let match_ = match_.lock().await;
                        let match_ = match &*match_ {
                            Some(match_) => match_,
                            _ => {
                                return;
                            }
                        };

                        let mut round_state = match_.lock_round_state().await;
                        round_state.set_last_result(battle::BattleResult::Win);
                    });
                })
            }),
            (self.offsets.rom.round_end_set_loss, {
                let match_ = match_.clone();
                Box::new(move |_| {
                    tokio::runtime::Handle::current().block_on(async {
                        let match_ = match_.lock().await;
                        let match_ = match &*match_ {
                            Some(match_) => match_,
                            _ => {
                                return;
                            }
                        };

                        let mut round_state = match_.lock_round_state().await;
                        round_state.set_last_result(battle::BattleResult::Loss);
                    });
                })
            }),
            (self.offsets.rom.round_end_damage_judge_set_win, {
                let match_ = match_.clone();
                Box::new(move |_| {
                    tokio::runtime::Handle::current().block_on(async {
                        let match_ = match_.lock().await;
                        let match_ = match &*match_ {
                            Some(match_) => match_,
                            _ => {
                                return;
                            }
                        };

                        let mut round_state = match_.lock_round_state().await;
                        round_state.set_last_result(battle::BattleResult::Win);
                    });
                })
            }),
            (self.offsets.rom.round_end_damage_judge_set_loss, {
                let match_ = match_.clone();
                Box::new(move |_| {
                    tokio::runtime::Handle::current().block_on(async {
                        let match_ = match_.lock().await;
                        let match_ = match &*match_ {
                            Some(match_) => match_,
                            _ => {
                                return;
                            }
                        };

                        let mut round_state = match_.lock_round_state().await;
                        round_state.set_last_result(battle::BattleResult::Loss);
                    });
                })
            }),
            (self.offsets.rom.round_end_damage_judge_set_draw, {
                let match_ = match_.clone();
                Box::new(move |_| {
                    tokio::runtime::Handle::current().block_on(async {
                        let match_ = match_.lock().await;
                        let match_ = match &*match_ {
                            Some(match_) => match_,
                            _ => {
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
                })
            }),
            (self.offsets.rom.round_set_ending, {
                let match_ = match_.clone();
                Box::new(move |_| {
                    tokio::runtime::Handle::current().block_on(async {
                        let match_ = match_.lock().await;
                        let match_ = match &*match_ {
                            Some(match_) => match_,
                            _ => {
                                return;
                            }
                        };

                        let mut round_state = match_.lock_round_state().await;
                        round_state.end_round().await.expect("end round");
                        match_.advance_shadow_until_round_end().await.expect("advance shadow");
                    });
                })
            }),
            (self.offsets.rom.round_start_ret, {
                let match_ = match_.clone();
                Box::new(move |_core| {
                    tokio::runtime::Handle::current().block_on(async {
                        let match_ = match_.lock().await;
                        let match_ = match &*match_ {
                            Some(match_) => match_,
                            _ => {
                                return;
                            }
                        };
                        match_.start_round().await.expect("start round");
                    });
                })
            }),
            (self.offsets.rom.battle_is_p2_tst, {
                let match_ = match_.clone();
                Box::new(move |mut core| {
                    tokio::runtime::Handle::current().block_on(async {
                        let match_ = match_.lock().await;
                        let match_ = match &*match_ {
                            Some(match_) => match_,
                            _ => {
                                return;
                            }
                        };

                        let round_state = match_.lock_round_state().await;
                        let round = round_state.round.as_ref().expect("round");

                        core.gba_mut().cpu_mut().set_gpr(0, round.local_player_index() as i32);
                    });
                })
            }),
            (self.offsets.rom.link_is_p2_ret, {
                let match_ = match_.clone();
                Box::new(move |mut core| {
                    tokio::runtime::Handle::current().block_on(async {
                        let match_ = match_.lock().await;
                        let match_ = match &*match_ {
                            Some(match_) => match_,
                            _ => {
                                return;
                            }
                        };

                        let round_state = match_.lock_round_state().await;
                        let round = round_state.round.as_ref().expect("round");

                        core.gba_mut().cpu_mut().set_gpr(0, round.local_player_index() as i32);
                    });
                })
            }),
            (
                self.offsets.rom.handle_sio_entry,
                Box::new(move |core| {
                    log::error!(
                        "unhandled call to handleSIO at 0x{:0x}: uh oh!",
                        core.as_ref().gba().cpu().gpr(14) - 2
                    );
                }),
            ),
            (self.offsets.rom.comm_menu_init_battle_entry, {
                let match_ = match_.clone();
                let munger = self.munger();
                Box::new(move |core| {
                    tokio::runtime::Handle::current().block_on(async {
                        let match_ = match_.lock().await;
                        let match_ = match &*match_ {
                            Some(match_) => match_,
                            _ => {
                                return;
                            }
                        };

                        let mut rng = match_.lock_rng().await;
                        let (battle_settings, background) =
                            random_battle_settings_and_background(match_.match_type().1 == 1, &mut *rng);
                        munger.set_battle_settings_and_background(core, battle_settings, background);
                    });
                })
            }),
            (
                self.offsets.rom.comm_menu_end_battle_entry,
                Box::new(move |_core| {
                    completion_token.complete();
                }),
            ),
            (self.offsets.rom.in_battle_call_handle_link_cable_input, {
                let match_ = match_.clone();
                let munger = self.munger();
                Box::new(move |mut core| {
                    tokio::runtime::Handle::current().block_on(async {
                        let pc = core.as_ref().gba().cpu().thumb_pc() as u32;
                        core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
                        munger.set_copy_data_input_state(core, if match_.lock().await.is_some() { 2 } else { 4 });
                    });
                })
            }),
            (self.offsets.rom.main_read_joyflags, {
                let match_ = match_.clone();
                let munger = self.munger();
                Box::new(move |core| {
                    tokio::runtime::Handle::current().block_on(async {
                        let match_ = match_.lock().await;
                        let match_ = match &*match_ {
                            Some(match_) => match_,
                            _ => {
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

                        'abort: loop {
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
                        match_.cancel();
                    });
                })
            }),
            (self.offsets.rom.round_post_increment_tick, {
                let match_ = match_.clone();
                let munger = self.munger();
                Box::new(move |core| {
                    tokio::runtime::Handle::current().block_on(async {
                        let match_ = match_.lock().await;
                        let match_ = match &*match_ {
                            Some(match_) => match_,
                            _ => {
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
                })
            }),
        ]
    }

    fn shadow_traps(&self, shadow_state: shadow::State) -> Vec<(u32, Box<dyn Fn(mgba::core::CoreMutRef)>)> {
        vec![
            (self.offsets.rom.comm_menu_init_ret, {
                let munger = self.munger();
                let shadow_state = shadow_state.clone();
                Box::new(move |core| {
                    munger.start_battle_from_comm_menu(core, shadow_state.match_type().0);
                })
            }),
            (self.offsets.rom.round_end_set_win, {
                let shadow_state = shadow_state.clone();
                Box::new(move |_| {
                    let mut round_state = shadow_state.lock_round_state();
                    round_state.set_last_result(battle::BattleResult::Loss);
                })
            }),
            (self.offsets.rom.round_end_set_loss, {
                let shadow_state = shadow_state.clone();
                Box::new(move |_| {
                    let mut round_state = shadow_state.lock_round_state();
                    round_state.set_last_result(battle::BattleResult::Win);
                })
            }),
            (self.offsets.rom.round_end_damage_judge_set_win, {
                let shadow_state = shadow_state.clone();
                Box::new(move |_| {
                    let mut round_state = shadow_state.lock_round_state();
                    round_state.set_last_result(battle::BattleResult::Loss);
                })
            }),
            (self.offsets.rom.round_end_damage_judge_set_loss, {
                let shadow_state = shadow_state.clone();
                Box::new(move |_| {
                    let mut round_state = shadow_state.lock_round_state();
                    round_state.set_last_result(battle::BattleResult::Win);
                })
            }),
            (self.offsets.rom.round_end_damage_judge_set_draw, {
                let shadow_state = shadow_state.clone();
                Box::new(move |_| {
                    let mut round_state = shadow_state.lock_round_state();
                    let result = {
                        let round = round_state.round.as_mut().expect("round");
                        round.on_draw_result()
                    };
                    round_state.set_last_result(result);
                })
            }),
            (self.offsets.rom.round_start_ret, {
                let shadow_state = shadow_state.clone();
                Box::new(move |_| {
                    shadow_state.start_round();
                })
            }),
            (self.offsets.rom.round_end_entry, {
                let shadow_state = shadow_state.clone();
                Box::new(move |core| {
                    shadow_state.end_round();
                    shadow_state.set_applied_state(core.save_state().expect("save state"), 0);
                })
            }),
            (self.offsets.rom.battle_is_p2_tst, {
                let shadow_state = shadow_state.clone();
                Box::new(move |mut core| {
                    let mut round_state = shadow_state.lock_round_state();
                    let round = round_state.round.as_mut().expect("round");

                    core.gba_mut().cpu_mut().set_gpr(0, round.remote_player_index() as i32);
                })
            }),
            (self.offsets.rom.link_is_p2_ret, {
                let shadow_state = shadow_state.clone();
                Box::new(move |mut core| {
                    let mut round_state = shadow_state.lock_round_state();
                    let round = round_state.round.as_mut().expect("round");

                    core.gba_mut().cpu_mut().set_gpr(0, round.remote_player_index() as i32);
                })
            }),
            (
                self.offsets.rom.handle_sio_entry,
                Box::new(move |core| {
                    log::error!(
                        "unhandled call to handleSIO at 0x{:0x}: uh oh!",
                        core.as_ref().gba().cpu().gpr(14) - 2
                    );
                }),
            ),
            (self.offsets.rom.comm_menu_init_battle_entry, {
                let shadow_state = shadow_state.clone();
                let munger = self.munger();
                Box::new(move |core| {
                    let mut rng = shadow_state.lock_rng();
                    let (battle_settings, background) =
                        random_battle_settings_and_background(shadow_state.match_type().1 == 1, &mut *rng);
                    munger.set_battle_settings_and_background(core, battle_settings, background);
                })
            }),
            (self.offsets.rom.in_battle_call_handle_link_cable_input, {
                let munger = self.munger();
                Box::new(move |mut core| {
                    let pc = core.as_ref().gba().cpu().thumb_pc() as u32;
                    core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
                    munger.set_copy_data_input_state(core, 2);
                })
            }),
            (self.offsets.rom.main_read_joyflags, {
                let shadow_state = shadow_state.clone();
                let munger = self.munger();
                Box::new(move |mut core| {
                    let mut round_state = shadow_state.lock_round_state();
                    let round = match round_state.round.as_mut() {
                        Some(round) => round,
                        None => {
                            return;
                        }
                    };

                    if !round.has_first_committed_state() {
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

                        // HACK: The battle jump table goes directly from deinit to init, so we actually end up initializing on tick 1 after round 1. We just override it here.
                        munger.set_current_tick(core, 0);

                        round
                            .set_first_committed_state(core.save_state().expect("save state"), &munger.tx_packet(core));
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
                        shadow_state.set_applied_state(core.save_state().expect("save state"), round.current_tick());
                    }
                })
            }),
            (self.offsets.rom.copy_input_data_entry, {
                let shadow_state = shadow_state.clone();
                let munger = self.munger();
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
                })
            }),
            (self.offsets.rom.copy_input_data_ret, {
                let shadow_state = shadow_state.clone();
                let munger = self.munger();
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

                    round.set_remote_packet(round.current_tick() + 1, munger.tx_packet(core).to_vec());
                    round.set_input_injected();
                })
            }),
            (self.offsets.rom.round_post_increment_tick, {
                let shadow_state = shadow_state.clone();
                let munger = self.munger();
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
                })
            }),
        ]
    }

    fn replayer_traps(&self, replayer_state: replayer::State) -> Vec<(u32, Box<dyn Fn(mgba::core::CoreMutRef)>)> {
        vec![
            (self.offsets.rom.battle_start_play_music_call, {
                let replayer_state = replayer_state.clone();
                Box::new(move |mut core| {
                    let replayer_state = replayer_state.lock_inner();
                    if !replayer_state.disable_bgm() {
                        return;
                    }
                    let pc = core.as_ref().gba().cpu().thumb_pc() as u32;
                    core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
                })
            }),
            (self.offsets.rom.battle_is_p2_tst, {
                let replayer_state = replayer_state.clone();
                Box::new(move |mut core| {
                    let replayer_state = replayer_state.lock_inner();
                    core.gba_mut()
                        .cpu_mut()
                        .set_gpr(0, replayer_state.local_player_index() as i32);
                })
            }),
            (self.offsets.rom.link_is_p2_ret, {
                let replayer_state = replayer_state.clone();
                Box::new(move |mut core| {
                    let replayer_state = replayer_state.lock_inner();
                    core.gba_mut()
                        .cpu_mut()
                        .set_gpr(0, replayer_state.local_player_index() as i32);
                })
            }),
            (self.offsets.rom.in_battle_call_handle_link_cable_input, {
                let munger = self.munger();
                Box::new(move |mut core| {
                    let pc = core.as_ref().gba().cpu().thumb_pc() as u32;
                    core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
                    munger.set_copy_data_input_state(core, 2);
                })
            }),
            (self.offsets.rom.round_set_ending, {
                let replayer_state = replayer_state.clone();
                Box::new(move |_core| {
                    let mut replayer_state = replayer_state.lock_inner();
                    replayer_state.set_round_ending();
                })
            }),
            (self.offsets.rom.round_end_entry, {
                let replayer_state = replayer_state.clone();
                Box::new(move |_core| {
                    let mut replayer_state = replayer_state.lock_inner();
                    replayer_state.set_round_ended();
                })
            }),
            (self.offsets.rom.main_read_joyflags, {
                let munger = self.munger();
                let replayer_state = replayer_state.clone();
                Box::new(move |mut core| {
                    let mut replayer_state = replayer_state.lock_inner();
                    let current_tick = replayer_state.current_tick();

                    let game_current_tick = munger.current_tick(core);
                    if game_current_tick != current_tick {
                        panic!("round tick = {} but game tick = {}", current_tick, game_current_tick);
                    }

                    if current_tick == replayer_state.commit_tick() {
                        replayer_state.set_committed_state(core.save_state().expect("save committed state"));
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

                    core.gba_mut().cpu_mut().set_gpr(4, (ip.local.joyflags | 0xfc00) as i32);

                    if current_tick == replayer_state.dirty_tick() {
                        replayer_state.set_dirty_state(core.save_state().expect("save dirty state"));
                    }
                })
            }),
            (self.offsets.rom.copy_input_data_entry, {
                let munger = self.munger();
                let replayer_state = replayer_state.clone();
                Box::new(move |core| {
                    let mut replayer_state = replayer_state.lock_inner();
                    if replayer_state.is_round_ending() {
                        return;
                    }

                    let current_tick = replayer_state.current_tick();

                    let game_current_tick = munger.current_tick(core);
                    if game_current_tick != current_tick {
                        panic!("round tick = {} but game tick = {}", current_tick, game_current_tick);
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
                            .apply_shadow_input(lockstep::Pair {
                                local: ip.local.with_packet(local_packet.packet),
                                remote: ip.remote,
                            })
                            .expect("apply shadow input")
                            .try_into()
                            .unwrap(),
                    );
                })
            }),
            (self.offsets.rom.copy_input_data_ret, {
                let munger = self.munger();
                let replayer_state = replayer_state.clone();
                Box::new(move |core| {
                    let mut replayer_state = replayer_state.lock_inner();
                    if replayer_state.is_round_ending() {
                        return;
                    }

                    let current_tick = replayer_state.current_tick();

                    let game_current_tick = munger.current_tick(core);
                    if game_current_tick != current_tick {
                        panic!("round tick = {} but game tick = {}", current_tick, game_current_tick);
                    }

                    replayer_state.set_local_packet(current_tick + 1, munger.tx_packet(core).to_vec());
                })
            }),
            (self.offsets.rom.round_post_increment_tick, {
                let replayer_state = replayer_state.clone();
                let munger = self.munger();
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
                })
            }),
            (self.offsets.rom.round_end_set_win, {
                let replayer_state = replayer_state.clone();
                Box::new(move |_| {
                    let mut replayer_state = replayer_state.lock_inner();
                    replayer_state.set_round_result(replayer::BattleResult::Win);
                })
            }),
            (self.offsets.rom.round_end_set_loss, {
                let replayer_state = replayer_state.clone();
                Box::new(move |_| {
                    let mut replayer_state = replayer_state.lock_inner();
                    replayer_state.set_round_result(replayer::BattleResult::Loss);
                })
            }),
            (self.offsets.rom.round_end_damage_judge_set_win, {
                let replayer_state = replayer_state.clone();
                Box::new(move |_| {
                    let mut replayer_state = replayer_state.lock_inner();
                    replayer_state.set_round_result(replayer::BattleResult::Win);
                })
            }),
            (self.offsets.rom.round_end_damage_judge_set_loss, {
                let replayer_state = replayer_state.clone();
                Box::new(move |_| {
                    let mut replayer_state = replayer_state.lock_inner();
                    replayer_state.set_round_result(replayer::BattleResult::Loss);
                })
            }),
            (self.offsets.rom.round_end_damage_judge_set_draw, {
                let replayer_state = replayer_state.clone();
                Box::new(move |_| {
                    let mut replayer_state = replayer_state.lock_inner();
                    replayer_state.set_round_result(replayer::BattleResult::Draw);
                })
            }),
        ]
    }

    fn prepare_for_fastforward(&self, mut core: mgba::core::CoreMutRef) {
        core.gba_mut()
            .cpu_mut()
            .set_thumb_pc(self.offsets.rom.main_read_joyflags);
    }
}
