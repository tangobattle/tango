mod munger;
mod offsets;

use crate::{battle, game, lockstep, replayer, session, shadow, sync};

pub struct Hooks {
    offsets: &'static offsets::Offsets,
}

impl Hooks {
    fn munger(&self) -> munger::Munger {
        munger::Munger { offsets: self.offsets }
    }
}

pub static B4BE_00: Hooks = Hooks {
    offsets: &offsets::B4BE_00,
};
pub static B4WE_00: Hooks = Hooks {
    offsets: &offsets::B4WE_00,
};

#[allow(dead_code)]
pub static B4BJ_00: Hooks = Hooks {
    offsets: &offsets::B4BJ_00,
};

pub static B4BJ_01: Hooks = Hooks {
    offsets: &offsets::B4BJ_01,
};

#[allow(dead_code)]
pub static B4WJ_00: Hooks = Hooks {
    offsets: &offsets::B4WJ_00,
};

pub static B4WJ_01: Hooks = Hooks {
    offsets: &offsets::B4WJ_01,
};

fn step_rng(seed: u32) -> u32 {
    let seed = std::num::Wrapping(seed);
    ((seed << 1) + (seed >> 0x1f) + std::num::Wrapping(1)).0 ^ 0x873ca9e5
}

fn generate_rng1_state(rng: &mut impl rand::Rng) -> u32 {
    (0..rng.gen_range(0..0x100000)).fold(0, |acc, _| step_rng(acc))
}

fn generate_rng2_state(rng: &mut impl rand::Rng) -> u32 {
    (0..rng.gen_range(0..0x100000)).fold(0xa338244f, |acc, _| step_rng(acc))
}

fn random_battle_settings_and_background(rng: &mut impl rand::Rng, match_type: (u8, u8)) -> (u8, u8) {
    (
        match match_type.0 {
            0 => rng.gen_range(0..0x44),
            1 => rng.gen_range(0..0x60),
            2 => rng.gen_range(0..0x44),
            _ => 0,
        },
        match match_type.1 {
            0 => rng.gen_range(0..0x18),
            1 => rng.gen_range(0x18..0x1b),
            _ => 0,
        },
    )
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
            (self.offsets.rom.ngplus_menu_init_ret, {
                let munger = self.munger();

                Box::new(move |core| {
                    munger.continue_from_ngplus_menu(core);
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
            (self.offsets.rom.battle_pizzazz_init_mov, {
                let match_ = match_.clone();
                Box::new(move |mut core| {
                    let match_ = sync::block_on(match_.lock());
                    let match_ = match &*match_ {
                        Some(match_) => match_,
                        _ => {
                            return;
                        }
                    };

                    if match_.match_type().1 != 1 {
                        return;
                    }

                    let v = core.as_ref().gba().cpu().gpr(0) | 0x20;
                    core.gba_mut().cpu_mut().set_gpr(0, v);
                })
            }),
            (self.offsets.rom.battle_pizzazz_bg_mov, {
                let match_ = match_.clone();
                Box::new(move |mut core| {
                    let match_ = sync::block_on(match_.lock());
                    let match_ = match &*match_ {
                        Some(match_) => match_,
                        _ => {
                            return;
                        }
                    };

                    if match_.match_type().1 != 1 {
                        return;
                    }

                    let v = core.as_ref().gba().cpu().gpr(1) | 0x20;
                    core.gba_mut().cpu_mut().set_gpr(1, v);
                })
            }),
            (self.offsets.rom.battle_pizzazz_self_mov, {
                let match_ = match_.clone();
                Box::new(move |mut core| {
                    let match_ = sync::block_on(match_.lock());
                    let match_ = match &*match_ {
                        Some(match_) => match_,
                        _ => {
                            return;
                        }
                    };

                    if match_.match_type().1 != 1 {
                        return;
                    }

                    let v = core.as_ref().gba().cpu().gpr(1) | 0x20;
                    core.gba_mut().cpu_mut().set_gpr(1, v);
                })
            }),
            (self.offsets.rom.battle_pizzazz_opponent_mov, {
                let match_ = match_.clone();
                Box::new(move |mut core| {
                    let match_ = sync::block_on(match_.lock());
                    let match_ = match &*match_ {
                        Some(match_) => match_,
                        _ => {
                            return;
                        }
                    };

                    if match_.match_type().1 != 1 {
                        return;
                    }

                    let v = core.as_ref().gba().cpu().gpr(0) | 0x20;
                    core.gba_mut().cpu_mut().set_gpr(0, v);
                })
            }),
            (self.offsets.rom.battle_pizzazz_silhouette_mov, {
                let match_ = match_.clone();
                Box::new(move |mut core| {
                    let match_ = sync::block_on(match_.lock());
                    let match_ = match &*match_ {
                        Some(match_) => match_,
                        _ => {
                            return;
                        }
                    };

                    if match_.match_type().1 != 1 {
                        return;
                    }

                    let v = core.as_ref().gba().cpu().gpr(0) | 0x20;
                    core.gba_mut().cpu_mut().set_gpr(0, v);
                })
            }),
            (self.offsets.rom.battle_pizzazz_final_mov, {
                let match_ = match_.clone();
                Box::new(move |mut core| {
                    let match_ = sync::block_on(match_.lock());
                    let match_ = match &*match_ {
                        Some(match_) => match_,
                        _ => {
                            return;
                        }
                    };

                    if match_.match_type().1 != 1 {
                        return;
                    }

                    let v = core.as_ref().gba().cpu().gpr(1) | 0x20;
                    core.gba_mut().cpu_mut().set_gpr(1, v);
                })
            }),
            (self.offsets.rom.comm_menu_init_ret, {
                let match_ = match_.clone();
                let munger = self.munger();
                Box::new(move |core| {
                    let match_ = sync::block_on(match_.lock());
                    let match_ = match &*match_ {
                        Some(match_) => match_,
                        _ => {
                            return;
                        }
                    };

                    let mut rng = sync::block_on(match_.lock_rng());

                    let (battle_settings, background) =
                        random_battle_settings_and_background(&mut *rng, match_.match_type());

                    munger.start_battle_from_comm_menu(core, match_.match_type().0, battle_settings, background);
                })
            }),
            (
                self.offsets.rom.match_end_ret,
                Box::new(move |_core| {
                    completion_token.complete();
                }),
            ),
            (self.offsets.rom.round_end_set_win, {
                let match_ = match_.clone();
                Box::new(move |_| {
                    let match_ = sync::block_on(match_.lock());
                    let match_ = match &*match_ {
                        Some(match_) => match_,
                        _ => {
                            return;
                        }
                    };

                    let mut round_state = sync::block_on(match_.lock_round_state());
                    round_state.set_last_result(battle::BattleResult::Win);
                })
            }),
            (self.offsets.rom.round_end_set_loss, {
                let match_ = match_.clone();
                Box::new(move |_| {
                    let match_ = sync::block_on(match_.lock());
                    let match_ = match &*match_ {
                        Some(match_) => match_,
                        _ => {
                            return;
                        }
                    };

                    let mut round_state = sync::block_on(match_.lock_round_state());
                    round_state.set_last_result(battle::BattleResult::Loss);
                })
            }),
            (self.offsets.rom.round_end_damage_judge_set_win, {
                let match_ = match_.clone();
                Box::new(move |_| {
                    let match_ = sync::block_on(match_.lock());
                    let match_ = match &*match_ {
                        Some(match_) => match_,
                        _ => {
                            return;
                        }
                    };

                    let mut round_state = sync::block_on(match_.lock_round_state());
                    round_state.set_last_result(battle::BattleResult::Win);
                })
            }),
            (self.offsets.rom.round_end_damage_judge_set_loss, {
                let match_ = match_.clone();
                Box::new(move |_| {
                    let match_ = sync::block_on(match_.lock());
                    let match_ = match &*match_ {
                        Some(match_) => match_,
                        _ => {
                            return;
                        }
                    };

                    let mut round_state = sync::block_on(match_.lock_round_state());
                    round_state.set_last_result(battle::BattleResult::Loss);
                })
            }),
            (self.offsets.rom.round_end_damage_judge_set_draw, {
                let match_ = match_.clone();
                Box::new(move |_| {
                    let match_ = sync::block_on(match_.lock());
                    let match_ = match &*match_ {
                        Some(match_) => match_,
                        _ => {
                            return;
                        }
                    };

                    let mut round_state = sync::block_on(match_.lock_round_state());
                    let result = {
                        let round = round_state.round.as_ref().expect("round");
                        round.on_draw_result()
                    };
                    round_state.set_last_result(result);
                })
            }),
            (self.offsets.rom.round_set_ending, {
                let match_ = match_.clone();
                Box::new(move |_| {
                    let match_ = sync::block_on(match_.lock());
                    let match_ = match &*match_ {
                        Some(match_) => match_,
                        _ => {
                            return;
                        }
                    };

                    let mut round_state = sync::block_on(match_.lock_round_state());
                    sync::block_on(round_state.end_round()).expect("end round");
                    sync::block_on(match_.advance_shadow_until_round_end()).expect("advance shadow");
                })
            }),
            (self.offsets.rom.round_start_ret, {
                let match_ = match_.clone();
                Box::new(move |_core| {
                    let match_ = sync::block_on(match_.lock());
                    let match_ = match &*match_ {
                        Some(match_) => match_,
                        _ => {
                            return;
                        }
                    };
                    sync::block_on(match_.start_round()).expect("start round");
                })
            }),
            (self.offsets.rom.battle_is_p2_tst, {
                let match_ = match_.clone();
                Box::new(move |mut core| {
                    let match_ = sync::block_on(match_.lock());
                    let match_ = match &*match_ {
                        Some(match_) => match_,
                        _ => {
                            return;
                        }
                    };

                    let round_state = sync::block_on(match_.lock_round_state());
                    let round = round_state.round.as_ref().expect("round");

                    core.gba_mut().cpu_mut().set_gpr(0, round.local_player_index() as i32);
                })
            }),
            (self.offsets.rom.link_is_p2_ret, {
                let match_ = match_.clone();
                Box::new(move |mut core| {
                    let match_ = sync::block_on(match_.lock());
                    let match_ = match &*match_ {
                        Some(match_) => match_,
                        _ => {
                            return;
                        }
                    };

                    let round_state = sync::block_on(match_.lock_round_state());
                    let round = round_state.round.as_ref().expect("round");

                    core.gba_mut().cpu_mut().set_gpr(0, round.local_player_index() as i32);
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
            (self.offsets.rom.in_battle_call_handle_link_cable_input, {
                let match_ = match_.clone();
                let munger = self.munger();
                Box::new(move |mut core| {
                    let pc = core.as_ref().gba().cpu().thumb_pc() as u32;
                    core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
                    munger.set_copy_data_input_state(core, if sync::block_on(match_.lock()).is_some() { 2 } else { 4 });
                })
            }),
            (self.offsets.rom.main_read_joyflags, {
                let match_ = match_.clone();
                let munger = self.munger();
                Box::new(move |core| {
                    let match_ = sync::block_on(match_.lock());
                    let match_ = match &*match_ {
                        Some(match_) => match_,
                        _ => {
                            return;
                        }
                    };

                    let mut round_state = sync::block_on(match_.lock_round_state());

                    let round = match round_state.round.as_mut() {
                        Some(round) => round,
                        None => {
                            return;
                        }
                    };

                    if !round.has_committed_state() {
                        let mut rng = sync::block_on(match_.lock_rng());

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

                        round.set_first_committed_state(
                            core.save_state().expect("save state"),
                            sync::block_on(match_.advance_shadow_until_first_committed_state())
                                .expect("shadow save state"),
                            &munger.tx_packet(core),
                        );
                        log::info!(
                            "primary rng1 state: {:08x}, rng2 state: {:08x}",
                            munger.rng1_state(core),
                            munger.rng2_state(core),
                        );
                        log::info!("battle state committed on {}", round.current_tick());
                    }

                    if let Err(e) = sync::block_on(round.add_local_input_and_fastforward(
                        core,
                        joyflags.load(std::sync::atomic::Ordering::Relaxed) as u16,
                    )) {
                        log::error!("failed to add local input: {}", e);
                        match_.cancel();
                    }
                })
            }),
            (self.offsets.rom.round_call_jump_table_ret, {
                let match_ = match_.clone();
                Box::new(move |_core| {
                    let match_ = sync::block_on(match_.lock());
                    let match_ = match &*match_ {
                        Some(match_) => match_,
                        _ => {
                            return;
                        }
                    };

                    let mut round_state = sync::block_on(match_.lock_round_state());
                    let round = if let Some(round) = round_state.round.as_mut() {
                        round
                    } else {
                        return;
                    };

                    if !round.has_committed_state() {
                        return;
                    }

                    round.increment_current_tick();
                })
            }),
        ]
    }

    fn shadow_traps(&self, shadow_state: shadow::State) -> Vec<(u32, Box<dyn Fn(mgba::core::CoreMutRef)>)> {
        vec![
            (self.offsets.rom.battle_pizzazz_init_mov, {
                let shadow_state = shadow_state.clone();
                Box::new(move |mut core| {
                    if shadow_state.match_type().1 != 1 {
                        return;
                    }
                    let v = core.as_ref().gba().cpu().gpr(0) | 0x20;
                    core.gba_mut().cpu_mut().set_gpr(0, v);
                })
            }),
            (self.offsets.rom.battle_pizzazz_bg_mov, {
                let shadow_state = shadow_state.clone();
                Box::new(move |mut core| {
                    if shadow_state.match_type().1 != 1 {
                        return;
                    }
                    let v = core.as_ref().gba().cpu().gpr(1) | 0x20;
                    core.gba_mut().cpu_mut().set_gpr(1, v);
                })
            }),
            (self.offsets.rom.battle_pizzazz_self_mov, {
                let shadow_state = shadow_state.clone();
                Box::new(move |mut core| {
                    if shadow_state.match_type().1 != 1 {
                        return;
                    }
                    let v = core.as_ref().gba().cpu().gpr(1) | 0x20;
                    core.gba_mut().cpu_mut().set_gpr(1, v);
                })
            }),
            (self.offsets.rom.battle_pizzazz_opponent_mov, {
                let shadow_state = shadow_state.clone();
                Box::new(move |mut core| {
                    if shadow_state.match_type().1 != 1 {
                        return;
                    }
                    let v = core.as_ref().gba().cpu().gpr(0) | 0x20;
                    core.gba_mut().cpu_mut().set_gpr(0, v);
                })
            }),
            (self.offsets.rom.battle_pizzazz_silhouette_mov, {
                let shadow_state = shadow_state.clone();
                Box::new(move |mut core| {
                    if shadow_state.match_type().1 != 1 {
                        return;
                    }
                    let v = core.as_ref().gba().cpu().gpr(0) | 0x20;
                    core.gba_mut().cpu_mut().set_gpr(0, v);
                })
            }),
            (self.offsets.rom.battle_pizzazz_final_mov, {
                let shadow_state = shadow_state.clone();
                Box::new(move |mut core| {
                    if shadow_state.match_type().1 != 1 {
                        return;
                    }
                    let v = core.as_ref().gba().cpu().gpr(1) | 0x20;
                    core.gba_mut().cpu_mut().set_gpr(1, v);
                })
            }),
            (self.offsets.rom.comm_menu_init_ret, {
                let munger = self.munger();
                let shadow_state = shadow_state.clone();
                Box::new(move |core| {
                    let mut rng = shadow_state.lock_rng();

                    let (battle_settings, background) =
                        random_battle_settings_and_background(&mut *rng, shadow_state.match_type());

                    munger.start_battle_from_comm_menu(core, shadow_state.match_type().0, battle_settings, background);
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

                        // HACK: For some inexplicable reason, we don't always start on tick 0.
                        round
                            .set_first_committed_state(core.save_state().expect("save state"), &munger.tx_packet(core));
                        log::info!(
                            "shadow rng1 state: {:08x}, rng2 state: {:08x}",
                            munger.rng1_state(core),
                            munger.rng2_state(core),
                        );
                        log::info!("shadow state committed on {}", round.current_tick());
                        return;
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
                    round.set_remote_packet(round.current_tick() + 1, munger.tx_packet(core).to_vec());
                    round.set_input_injected();
                })
            }),
            (self.offsets.rom.round_call_jump_table_ret, {
                let shadow_state = shadow_state.clone();
                Box::new(move |_core| {
                    let mut round_state = shadow_state.lock_round_state();
                    let round = if let Some(round) = round_state.round.as_mut() {
                        round
                    } else {
                        return;
                    };
                    if !round.has_first_committed_state() {
                        return;
                    }
                    round.increment_current_tick();
                })
            }),
        ]
    }

    fn replayer_traps(&self, replayer_state: replayer::State) -> Vec<(u32, Box<dyn Fn(mgba::core::CoreMutRef)>)> {
        vec![
            (self.offsets.rom.battle_pizzazz_init_mov, {
                let replayer_state = replayer_state.clone();
                Box::new(move |mut core| {
                    let replayer_state = replayer_state.lock_inner();
                    if replayer_state.match_type().1 != 1 {
                        return;
                    }
                    let v = core.as_ref().gba().cpu().gpr(0) | 0x20;
                    core.gba_mut().cpu_mut().set_gpr(0, v);
                })
            }),
            (self.offsets.rom.battle_pizzazz_bg_mov, {
                let replayer_state = replayer_state.clone();
                Box::new(move |mut core| {
                    let replayer_state = replayer_state.lock_inner();
                    if replayer_state.match_type().1 != 1 {
                        return;
                    }
                    let v = core.as_ref().gba().cpu().gpr(1) | 0x20;
                    core.gba_mut().cpu_mut().set_gpr(1, v);
                })
            }),
            (self.offsets.rom.battle_pizzazz_self_mov, {
                let replayer_state = replayer_state.clone();
                Box::new(move |mut core| {
                    let replayer_state = replayer_state.lock_inner();
                    if replayer_state.match_type().1 != 1 {
                        return;
                    }
                    let v = core.as_ref().gba().cpu().gpr(1) | 0x20;
                    core.gba_mut().cpu_mut().set_gpr(1, v);
                })
            }),
            (self.offsets.rom.battle_pizzazz_opponent_mov, {
                let replayer_state = replayer_state.clone();
                Box::new(move |mut core| {
                    let replayer_state = replayer_state.lock_inner();
                    if replayer_state.match_type().1 != 1 {
                        return;
                    }
                    let v = core.as_ref().gba().cpu().gpr(0) | 0x20;
                    core.gba_mut().cpu_mut().set_gpr(0, v);
                })
            }),
            (self.offsets.rom.battle_pizzazz_silhouette_mov, {
                let replayer_state = replayer_state.clone();
                Box::new(move |mut core| {
                    let replayer_state = replayer_state.lock_inner();
                    if replayer_state.match_type().1 != 1 {
                        return;
                    }
                    let v = core.as_ref().gba().cpu().gpr(0) | 0x20;
                    core.gba_mut().cpu_mut().set_gpr(0, v);
                })
            }),
            (self.offsets.rom.battle_pizzazz_final_mov, {
                let replayer_state = replayer_state.clone();
                Box::new(move |mut core| {
                    let replayer_state = replayer_state.lock_inner();
                    if replayer_state.match_type().1 != 1 {
                        return;
                    }
                    let v = core.as_ref().gba().cpu().gpr(1) | 0x20;
                    core.gba_mut().cpu_mut().set_gpr(1, v);
                })
            }),
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
                let replayer_state = replayer_state.clone();
                Box::new(move |mut core| {
                    let mut replayer_state = replayer_state.lock_inner();
                    let current_tick = replayer_state.current_tick();

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
                    replayer_state.set_local_packet(current_tick + 1, munger.tx_packet(core).to_vec());
                })
            }),
            (self.offsets.rom.round_call_jump_table_ret, {
                let replayer_state = replayer_state.clone();
                Box::new(move |_core| {
                    let mut replayer_state = replayer_state.lock_inner();
                    replayer_state.increment_current_tick();
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
