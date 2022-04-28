use rand::Rng;

use crate::{facade, fastforwarder, hooks};

mod munger;
mod offsets;

#[derive(Clone)]
pub struct BN6 {
    offsets: offsets::Offsets,
    munger: munger::Munger,
}

lazy_static! {
    pub static ref MEGAMAN6_FXX: Box<dyn hooks::Hooks + Send + Sync> =
        BN6::new(offsets::MEGAMAN6_FXX);
    pub static ref MEGAMAN6_GXX: Box<dyn hooks::Hooks + Send + Sync> =
        BN6::new(offsets::MEGAMAN6_GXX);
    pub static ref ROCKEXE6_RXX: Box<dyn hooks::Hooks + Send + Sync> =
        BN6::new(offsets::ROCKEXE6_RXX);
    pub static ref ROCKEXE6_GXX: Box<dyn hooks::Hooks + Send + Sync> =
        BN6::new(offsets::ROCKEXE6_GXX);
}

impl BN6 {
    pub fn new(offsets: offsets::Offsets) -> Box<dyn hooks::Hooks + Send + Sync> {
        Box::new(BN6 {
            offsets,
            munger: munger::Munger { offsets },
        })
    }
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
    ((seed * std::num::Wrapping(2)) - (seed >> 0x1f) + std::num::Wrapping(1)
        ^ std::num::Wrapping(0x873ca9e5))
    .0
}

impl hooks::Hooks for BN6 {
    fn primary_traps(
        &self,
        handle: tokio::runtime::Handle,
        facade: facade::Facade,
    ) -> Vec<(u32, Box<dyn FnMut(mgba::core::CoreMutRef)>)> {
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

                            // rng1 is the per-player rng, it does not need to be synced.
                            let mut rng1_state = 0;
                            for _ in 0..rand_pcg::Mcg128Xsl64::new(rand::thread_rng().gen())
                                .gen_range(0..=0xff)
                            {
                                rng1_state = step_rng(rng1_state);
                            }
                            munger.set_rng1_state(core, rng1_state);

                            // rng2 is the shared rng, it must be synced.
                            let mut rng = match_.lock_rng().await;
                            let mut rng2_state = 0xa338244f;
                            for _ in 0..rng.gen_range(0..=0xff) {
                                rng2_state = step_rng(rng2_state);
                            }
                            munger.set_rng2_state(core, rng2_state);
                        });
                    }),
                )
            },
            {
                let handle = handle.clone();
                (
                    self.offsets.rom.battle_init_call_battle_copy_input_data,
                    Box::new(move |mut core| {
                        handle.block_on(async {
                            core.gba_mut().cpu_mut().set_gpr(0, 0);
                            let r15 = core.as_ref().gba().cpu().gpr(15) as u32;
                            core.gba_mut().cpu_mut().set_pc(r15 + 4);
                        });
                    }),
                )
            },
            {
                let facade = facade.clone();
                let munger = self.munger.clone();
                let handle = handle.clone();
                (
                    self.offsets.rom.battle_init_tx_buf_copy_ret,
                    Box::new(move |core| {
                        handle.block_on(async {
                            'abort: loop {
                                let match_ = match facade.match_().await {
                                    Some(match_) => match_,
                                    None => {
                                        return;
                                    }
                                };

                                let mut battle_state = match_.lock_battle_state().await;

                                let local_init = munger.tx_buf(core);
                                battle_state.send_init(&local_init).await;
                                munger.set_rx_buf(
                                    core,
                                    battle_state.local_player_index() as u32,
                                    local_init.as_slice(),
                                );

                                let remote_init = match battle_state.receive_init().await {
                                    Some(remote_init) => remote_init,
                                    None => {
                                        break 'abort;
                                    }
                                };
                                munger.set_rx_buf(
                                    core,
                                    battle_state.remote_player_index() as u32,
                                    remote_init.as_slice(),
                                );
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
                    self.offsets.rom.battle_turn_tx_buf_copy_ret,
                    Box::new(move |core| {
                        handle.block_on(async {
                            let match_ = match facade.match_().await {
                                Some(match_) => match_,
                                None => {
                                    return;
                                }
                            };

                            let mut battle_state = match_.lock_battle_state().await;

                            log::info!("turn data marshaled on {}", munger.current_tick(core));
                            let local_turn = munger.tx_buf(core);
                            battle_state.add_local_pending_turn(local_turn);
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

                                let mut battle_state = match_.lock_battle_state().await;
                                if !battle_state.is_active() {
                                    return;
                                }

                                if !battle_state.is_accepting_input() {
                                    return;
                                }

                                let current_tick = munger.current_tick(core);
                                if !battle_state.has_committed_state() {
                                    battle_state
                                        .set_committed_state(core.save_state().expect("save state"))
                                        .await;
                                    battle_state.fill_input_delay(current_tick).await;
                                    log::info!("battle state committed");
                                }

                                let turn = battle_state.take_local_pending_turn();

                                if !battle_state
                                    .add_local_input_and_fastforward(
                                        core,
                                        current_tick,
                                        facade.joyflags() as u16,
                                        munger.local_custom_screen_state(core),
                                        turn.clone(),
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
                    self.offsets.rom.battle_update_call_battle_copy_input_data,
                    Box::new(move |mut core| {
                        handle.block_on(async {
                            core.gba_mut().cpu_mut().set_gpr(0, 0);
                            let r15 = core.as_ref().gba().cpu().gpr(15) as u32;
                            core.gba_mut().cpu_mut().set_pc(r15 + 4);

                            let match_ = match facade.match_().await {
                                Some(match_) => match_,
                                None => {
                                    return;
                                }
                            };

                            let mut battle_state = match_.lock_battle_state().await;
                            if !battle_state.is_active() {
                                return;
                            }

                            if !battle_state.is_accepting_input() {
                                battle_state.mark_accepting_input();
                                log::info!("battle is now accepting input");
                                return;
                            }

                            let ip = battle_state.take_last_input().expect("last input");

                            munger.set_player_input_state(
                                core,
                                battle_state.local_player_index() as u32,
                                ip.local.joyflags as u16,
                                ip.local.custom_screen_state as u8,
                            );
                            if !ip.local.turn.is_empty() {
                                munger.set_rx_buf(
                                    core,
                                    battle_state.local_player_index() as u32,
                                    ip.local.turn.as_slice(),
                                );
                            }
                            munger.set_player_input_state(
                                core,
                                battle_state.remote_player_index() as u32,
                                ip.remote.joyflags as u16,
                                ip.remote.custom_screen_state as u8,
                            );
                            if !ip.remote.turn.is_empty() {
                                munger.set_rx_buf(
                                    core,
                                    battle_state.remote_player_index() as u32,
                                    ip.remote.turn.as_slice(),
                                );
                            }
                        });
                    }),
                )
            },
            {
                let facade = facade.clone();
                let handle = handle.clone();
                (
                    self.offsets.rom.battle_run_unpaused_step_cmp_retval,
                    Box::new(move |core| {
                        handle.block_on(async {
                            let match_ = match facade.match_().await {
                                Some(match_) => match_,
                                None => {
                                    return;
                                }
                            };

                            let mut battle_state = match_.lock_battle_state().await;
                            if !battle_state.is_active() {
                                return;
                            }

                            if match core.as_ref().gba().cpu().gpr(0) {
                                1 => {
                                    battle_state.set_won_last_battle(true);
                                    true
                                }
                                2 => {
                                    battle_state.set_won_last_battle(false);
                                    true
                                }
                                _ => false,
                            } {
                                battle_state.end_battle().await;
                            }
                        });
                    }),
                )
            },
            {
                let facade = facade.clone();
                let handle = handle.clone();
                (
                    self.offsets.rom.battle_start_ret,
                    Box::new(move |core| {
                        handle.block_on(async {
                            let match_ = match facade.match_().await {
                                Some(match_) => match_,
                                None => {
                                    return;
                                }
                            };

                            match_.start_battle(core).await;
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

                            let battle_state = match_.lock_battle_state().await;
                            core.gba_mut()
                                .cpu_mut()
                                .set_gpr(0, battle_state.local_player_index() as i32);
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

                            let battle_state = match_.lock_battle_state().await;
                            core.gba_mut()
                                .cpu_mut()
                                .set_gpr(0, battle_state.local_player_index() as i32);
                        });
                    }),
                )
            },
            {
                let facade = facade.clone();
                let handle = handle.clone();
                (
                    self.offsets.rom.get_copy_data_input_state_ret,
                    Box::new(move |mut core| {
                        handle.block_on(async {
                            let mut r0 = core.as_ref().gba().cpu().gpr(0);
                            if r0 != 2 {
                                log::warn!("expected r0 to be 2 but got {}", r0);
                            }

                            if facade.match_().await.is_none() {
                                r0 = 4;
                            }

                            core.gba_mut().cpu_mut().set_gpr(0, r0);
                        });
                    }),
                )
            },
            {
                (
                    self.offsets.rom.comm_menu_handle_link_cable_input_entry,
                    Box::new(move |core| {
                        log::warn!(
                            "unhandled call to commMenu_handleLinkCableInput at 0x{:0x}: uh oh!",
                            core.as_ref().gba().cpu().gpr(15) - 4
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
                                    (match_.match_type() & 0xff) as u8,
                                ),
                            );
                        });
                    }),
                )
            },
            {
                let facade = facade.clone();
                let handle = handle;
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
                (
                    self.offsets
                        .rom
                        .comm_menu_in_battle_call_comm_menu_handle_link_cable_input,
                    Box::new(move |mut core| {
                        let r15 = core.as_ref().gba().cpu().gpr(15) as u32;
                        core.gba_mut().cpu_mut().set_pc(r15 + 4);
                    }),
                )
            },
        ]
    }

    fn fastforwarder_traps(
        &self,
        ff_state: fastforwarder::State,
    ) -> Vec<(u32, Box<dyn FnMut(mgba::core::CoreMutRef)>)> {
        vec![
            {
                let munger = self.munger.clone();
                let ff_state = ff_state.clone();
                (
                    self.offsets.rom.main_read_joyflags,
                    Box::new(move |mut core| {
                        let current_tick = munger.current_tick(core);

                        if current_tick == ff_state.commit_time() {
                            ff_state.set_committed_state(
                                core.save_state().expect("save committed state"),
                            );
                        }

                        let ip = match ff_state.peek_input_pair() {
                            Some(ip) => ip,
                            None => {
                                return;
                            }
                        };

                        if ip.local.local_tick != ip.remote.local_tick {
                            ff_state.set_anyhow_error(anyhow::anyhow!(
                                "p1 tick != p2 tick (in battle tick = {}): {} != {}",
                                current_tick,
                                ip.local.local_tick,
                                ip.remote.local_tick
                            ));
                            return;
                        }

                        if ip.local.local_tick != current_tick {
                            ff_state.set_anyhow_error(anyhow::anyhow!(
                                "input tick != in battle tick: {} != {}",
                                ip.local.local_tick,
                                current_tick,
                            ));
                            return;
                        }

                        core.gba_mut()
                            .cpu_mut()
                            .set_gpr(4, ip.local.joyflags as i32);

                        if current_tick == ff_state.dirty_time() {
                            ff_state.set_dirty_state(core.save_state().expect("save dirty state"));
                        }
                    }),
                )
            },
            {
                let munger = self.munger.clone();
                let ff_state = ff_state.clone();
                (
                    self.offsets.rom.battle_update_call_battle_copy_input_data,
                    Box::new(move |mut core| {
                        let current_tick = munger.current_tick(core);

                        let ip = match ff_state.pop_input_pair() {
                            Some(ip) => ip,
                            None => {
                                return;
                            }
                        };

                        core.gba_mut().cpu_mut().set_gpr(0, 0);
                        let r15 = core.as_ref().gba().cpu().gpr(15) as u32;
                        core.gba_mut().cpu_mut().set_pc(r15 + 4);

                        if ip.local.local_tick != ip.remote.local_tick {
                            ff_state.set_anyhow_error(anyhow::anyhow!(
                                "p1 tick != p2 tick (in battle tick = {}): {} != {}",
                                current_tick,
                                ip.local.local_tick,
                                ip.local.local_tick
                            ));
                            return;
                        }

                        if ip.local.local_tick != current_tick {
                            ff_state.set_anyhow_error(anyhow::anyhow!(
                                "input tick != in battle tick: {} != {}",
                                ip.local.local_tick,
                                current_tick,
                            ));
                            return;
                        }

                        let local_player_index = ff_state.local_player_index();
                        let remote_player_index = 1 - local_player_index;

                        munger.set_player_input_state(
                            core,
                            local_player_index as u32,
                            ip.local.joyflags,
                            ip.local.custom_screen_state,
                        );
                        if !ip.local.turn.is_empty() {
                            munger.set_rx_buf(
                                core,
                                local_player_index as u32,
                                ip.local.turn.as_slice(),
                            );
                        }

                        munger.set_player_input_state(
                            core,
                            remote_player_index as u32,
                            ip.remote.joyflags,
                            ip.remote.custom_screen_state,
                        );
                        if !ip.remote.turn.is_empty() {
                            munger.set_rx_buf(
                                core,
                                remote_player_index as u32,
                                ip.remote.turn.as_slice(),
                            );
                        }
                    }),
                )
            },
            {
                let ff_state = ff_state.clone();
                (
                    self.offsets.rom.battle_is_p2_tst,
                    Box::new(move |mut core| {
                        core.gba_mut()
                            .cpu_mut()
                            .set_gpr(0, ff_state.local_player_index() as i32);
                    }),
                )
            },
            {
                let ff_state = ff_state.clone();
                (
                    self.offsets.rom.link_is_p2_ret,
                    Box::new(move |mut core| {
                        core.gba_mut()
                            .cpu_mut()
                            .set_gpr(0, ff_state.local_player_index() as i32);
                    }),
                )
            },
            {
                (
                    self.offsets
                        .rom
                        .comm_menu_in_battle_call_comm_menu_handle_link_cable_input,
                    Box::new(move |mut core| {
                        let r15 = core.as_ref().gba().cpu().gpr(15) as u32;
                        core.gba_mut().cpu_mut().set_pc(r15 + 4);
                    }),
                )
            },
            {
                (
                    self.offsets.rom.get_copy_data_input_state_ret,
                    Box::new(move |mut core| {
                        core.gba_mut().cpu_mut().set_gpr(0, 2);
                    }),
                )
            },
            {
                let ff_state = ff_state.clone();
                (
                    self.offsets.rom.battle_end_entry,
                    Box::new(move |_core| {
                        ff_state.on_battle_ended();
                    }),
                )
            },
        ]
    }

    fn audio_traps(
        &self,
        facade: facade::AudioFacade,
    ) -> Vec<(u32, Box<dyn FnMut(mgba::core::CoreMutRef)>)> {
        vec![
            {
                let mut facade = facade.clone();
                (
                    self.offsets.rom.main_read_joyflags,
                    Box::new(move |mut core| {
                        let state = if let Some(state) = facade.take_audio_save_state() {
                            state
                        } else {
                            return;
                        };
                        core.load_state(&state).expect("loaded state");
                    }),
                )
            },
            {
                (
                    self.offsets.rom.battle_update_call_battle_copy_input_data,
                    Box::new(move |mut core| {
                        core.gba_mut().cpu_mut().set_gpr(0, 0);
                        let r15 = core.as_ref().gba().cpu().gpr(15) as u32;
                        core.gba_mut().cpu_mut().set_pc(r15 + 4);
                    }),
                )
            },
            {
                let facade = facade.clone();
                (
                    self.offsets.rom.battle_is_p2_tst,
                    Box::new(move |mut core| {
                        core.gba_mut()
                            .cpu_mut()
                            .set_gpr(0, facade.local_player_index() as i32);
                    }),
                )
            },
            {
                let facade = facade.clone();
                (
                    self.offsets.rom.link_is_p2_ret,
                    Box::new(move |mut core| {
                        core.gba_mut()
                            .cpu_mut()
                            .set_gpr(0, facade.local_player_index() as i32);
                    }),
                )
            },
            {
                (
                    self.offsets
                        .rom
                        .comm_menu_in_battle_call_comm_menu_handle_link_cable_input,
                    Box::new(move |mut core| {
                        let r15 = core.as_ref().gba().cpu().gpr(15) as u32;
                        core.gba_mut().cpu_mut().set_pc(r15 + 4);
                    }),
                )
            },
            {
                (
                    self.offsets.rom.get_copy_data_input_state_ret,
                    Box::new(move |mut core| {
                        core.gba_mut().cpu_mut().set_gpr(0, 2);
                    }),
                )
            },
        ]
    }

    fn prepare_for_fastforward(&self, mut core: mgba::core::CoreMutRef) {
        core.gba_mut()
            .cpu_mut()
            .set_pc(self.offsets.rom.main_read_joyflags);
    }

    fn current_tick(&self, core: mgba::core::CoreMutRef) -> u32 {
        self.munger.current_tick(core)
    }
}
