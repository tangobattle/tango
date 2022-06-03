mod munger;
mod offsets;

use crate::{facade, fastforwarder, hooks, input, shadow};

#[derive(Clone)]
pub struct BN4 {
    offsets: offsets::Offsets,
    munger: munger::Munger,
}

lazy_static! {
    pub static ref MEGAMANBN4BM: Box<dyn hooks::Hooks + Send + Sync> =
        BN4::new(offsets::MEGAMANBN4BM);
    // pub static ref MEGAMANBN4RS: Box<dyn hooks::Hooks + Send + Sync> =
    //     BN4::new(offsets::MEGAMANBN4RS);
    // pub static ref ROCK_EXE4_BM: Box<dyn hooks::Hooks + Send + Sync> = BN4::new();
    // pub static ref ROCK_EXE4_RS: Box<dyn hooks::Hooks + Send + Sync> = BN4::new();
}

impl BN4 {
    pub fn new(offsets: offsets::Offsets) -> Box<dyn hooks::Hooks + Send + Sync> {
        Box::new(BN4 {
            offsets,
            munger: munger::Munger { offsets },
        })
    }
}

fn step_rng(seed: u32) -> u32 {
    let seed = std::num::Wrapping(seed);
    (((seed * std::num::Wrapping(2)) - (seed >> 0x1f) + std::num::Wrapping(1))
        ^ std::num::Wrapping(0x873ca9e5))
    .0
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

impl hooks::Hooks for BN4 {
    fn primary_traps(
        &self,
        handle: tokio::runtime::Handle,
        joyflags: std::sync::Arc<std::sync::atomic::AtomicU32>,
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
                        log::info!("game loaded");
                        munger.open_comm_menu_from_overworld(core);
                    }),
                )
            },
            // TODO: comm_menu_init_ret
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
                                    round_state.set_won_last_round(true);
                                }
                                2 => {
                                    round_state.set_won_last_round(false);
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
                let facade = facade.clone();
                let handle = handle.clone();
                (
                    self.offsets.rom.get_copy_data_input_state_ret,
                    Box::new(move |mut core| {
                        handle.block_on(async {
                            let mut r0 = core.as_ref().gba().cpu().gpr(0);
                            if r0 != 2 {
                                log::error!("expected r0 to be 2 but got {}", r0);
                            }

                            if facade.match_().await.is_none() {
                                r0 = 4;
                            }

                            core.gba_mut().cpu_mut().set_gpr(0, r0);
                        });
                    }),
                )
            },
            // TODO: comm_menu_handle_link_cable_input_entry
            // TODO: comm_menu_end_battle_entry
            // TODO: comm_menu_in_battle_call_comm_menu_handle_link_cable_input
            // TODO: main_read_joyflags
            // TODO: round_phase_jump_table_ret
            {
                let facade = facade.clone();
                let handle = handle.clone();
                (
                    self.offsets.rom.round_phase_jump_table_ret,
                    Box::new(move |_core| {
                        handle.block_on(async {
                            let match_ = match facade.match_().await {
                                Some(match_) => match_,
                                None => {
                                    return;
                                }
                            };

                            let mut round_state = match_.lock_round_state().await;
                            let round = round_state.round.as_mut().expect("round");
                            round.increment_current_tick();
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
            // TODO: comm_menu_init_ret
            {
                let shadow_state = shadow_state.clone();
                (
                    self.offsets.rom.round_run_unpaused_step_cmp_retval,
                    Box::new(move |core| {
                        match core.as_ref().gba().cpu().gpr(0) {
                            1 => {
                                shadow_state.set_won_last_round(false);
                            }
                            2 => {
                                shadow_state.set_won_last_round(true);
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
                        shadow_state.set_applied_state(core.save_state().expect("save state"));
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
                    self.offsets.rom.get_copy_data_input_state_ret,
                    Box::new(move |core| {
                        let r0 = core.as_ref().gba().cpu().gpr(0);
                        if r0 != 2 {
                            log::error!("shadow: expected r0 to be 2 but got {}", r0);
                        }
                    }),
                )
            },
            // TODO: comm_menu_handle_link_cable_input_entry
            // TODO: comm_menu_init_battle_entry
            // TODO: comm_menu_in_battle_call_comm_menu_handle_link_cable_input
            // TODO: main_read_joyflags
            // TODO: copy_input_data_entry
            {
                let shadow_state = shadow_state.clone();
                (
                    self.offsets.rom.round_phase_jump_table_ret,
                    Box::new(move |_core| {
                        let mut round_state = shadow_state.lock_round_state();
                        let round = round_state.round.as_mut().expect("round");
                        round.increment_current_tick();
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
                    self.offsets.rom.round_end_entry,
                    Box::new(move |_core| {
                        ff_state.on_battle_ended();
                    }),
                )
            },
            // TODO: main_read_joyflags
            // TODO: copy_input_data_entry
            {
                let ff_state = ff_state.clone();
                (
                    self.offsets.rom.round_phase_jump_table_ret,
                    Box::new(move |_core| {
                        ff_state.increment_current_tick();
                    }),
                )
            },
        ]
    }

    fn placeholder_rx(&self) -> Vec<u8> {
        vec![0; 0x10]
    }

    fn prepare_for_fastforward(&self, mut core: mgba::core::CoreMutRef) {}

    fn replace_opponent_name(&self, mut core: mgba::core::CoreMutRef, name: &str) {}
}
