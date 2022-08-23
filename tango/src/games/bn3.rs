mod munger;
mod offsets;

use byteorder::ByteOrder;

use crate::{battle, games, lockstep, replayer, session, shadow};

#[derive(Clone)]
pub struct BN3 {
    offsets: offsets::Offsets,
    munger: munger::Munger,
}

lazy_static! {
    pub static ref MEGA_EXE3_BLA3XE_00: Box<dyn games::Hooks + Send + Sync> =
        BN3::new(offsets::MEGA_EXE3_BLA3XE_00);
    pub static ref MEGA_EXE3_WHA6BE_00: Box<dyn games::Hooks + Send + Sync> =
        BN3::new(offsets::MEGA_EXE3_WHA6BE_00);
    pub static ref ROCK_EXE3_BKA3XJ_01: Box<dyn games::Hooks + Send + Sync> =
        BN3::new(offsets::ROCK_EXE3_BKA3XJ_01);
    pub static ref ROCKMAN_EXE3A6BJ_01: Box<dyn games::Hooks + Send + Sync> =
        BN3::new(offsets::ROCKMAN_EXE3A6BJ_01);
}

impl BN3 {
    pub fn new(offsets: offsets::Offsets) -> Box<dyn games::Hooks + Send + Sync> {
        Box::new(BN3 {
            offsets,
            munger: munger::Munger { offsets },
        })
    }
}

fn bn3_match_type(rng: &mut impl rand::Rng, match_type: (u8, u8)) -> u8 {
    match match_type {
        (0, 1) => 0,
        (0, 2) => 1,
        (0, 3) => 2,
        (0, _) => rng.gen_range(0..3),
        (1, _) => 3,
        _ => 0,
    }
}

fn random_background(rng: &mut impl rand::Rng) -> u8 {
    const BATTLE_BACKGROUNDS: &[u8] = &[0x00, 0x04, 0x05, 0x06, 0x17, 0x10, 0x02, 0x0a];
    BATTLE_BACKGROUNDS[rng.gen_range(0..BATTLE_BACKGROUNDS.len())]
}

fn step_rng(seed: u32) -> u32 {
    let seed = std::num::Wrapping(seed);
    ((seed << 1) + (seed >> 0x1f) + std::num::Wrapping(1)).0 ^ 0x873ca9e5
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

const INIT_RX: [u8; 16] = [
    0x01, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
];

impl games::Hooks for BN3 {
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
        match_: std::sync::Arc<tokio::sync::Mutex<Option<std::sync::Arc<battle::Match>>>>,
        completion_token: session::CompletionToken,
    ) -> Vec<(u32, Box<dyn FnMut(mgba::core::CoreMutRef)>)> {
        let make_send_and_receive_call_hook = || {
            let match_ = match_.clone();
            let munger = self.munger.clone();
            let handle = handle.clone();
            Box::new(move |mut core: mgba::core::CoreMutRef| {
                handle.block_on(async {
                    let pc = core.as_ref().gba().cpu().thumb_pc();
                    core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);

                    let match_ = match_.lock().await;
                    let match_ = match &*match_ {
                        Some(match_) => match_,
                        _ => {
                            core.gba_mut().cpu_mut().set_gpr(0, 0);
                            return;
                        }
                    };
                    core.gba_mut().cpu_mut().set_gpr(0, 3);

                    let mut round_state = match_.lock_round_state().await;

                    let round = match round_state.round.as_mut() {
                        Some(round) => round,
                        None => {
                            return;
                        }
                    };

                    let current_tick = round.current_tick();
                    if current_tick > 1 {
                        let mut rx = [0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
                        byteorder::LittleEndian::write_u32(&mut rx[4..8], current_tick - 2);
                        munger.set_rx_packet(core, 0, &rx);
                        munger.set_rx_packet(core, 1, &rx);
                    }
                });
            })
        };

        vec![
            {
                let match_ = match_.clone();
                let handle = handle.clone();
                let munger = self.munger.clone();
                (
                    self.offsets.rom.comm_menu_init_ret,
                    Box::new(move |core| {
                        handle.block_on(async {
                            let match_ = match_.lock().await;
                            let match_ = match &*match_ {
                                Some(match_) => match_,
                                _ => {
                                    return;
                                }
                            };

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
                            munger.start_battle_from_comm_menu(
                                core,
                                bn3_match_type(&mut *rng, match_.match_type()),
                                random_background(&mut *rng),
                            );
                        });
                    }),
                )
            },
            {
                let handle = handle.clone();
                (
                    self.offsets.rom.match_end_ret,
                    Box::new(move |_core| {
                        handle.block_on(async {
                            completion_token.complete();
                        });
                    }),
                )
            },
            {
                let match_ = match_.clone();
                let handle = handle.clone();
                (
                    self.offsets.rom.round_end_set_win,
                    Box::new(move |_| {
                        handle.block_on(async {
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
                    }),
                )
            },
            {
                let match_ = match_.clone();
                let handle = handle.clone();
                (
                    self.offsets.rom.round_end_set_loss,
                    Box::new(move |_| {
                        handle.block_on(async {
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
                    }),
                )
            },
            {
                let match_ = match_.clone();
                let handle = handle.clone();
                (
                    self.offsets.rom.round_end_damage_judge_set_win,
                    Box::new(move |_| {
                        handle.block_on(async {
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
                    }),
                )
            },
            {
                let match_ = match_.clone();
                let handle = handle.clone();
                (
                    self.offsets.rom.round_end_damage_judge_set_loss,
                    Box::new(move |_| {
                        handle.block_on(async {
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
                    }),
                )
            },
            {
                let match_ = match_.clone();
                let handle = handle.clone();
                (
                    self.offsets.rom.round_end_damage_judge_set_draw,
                    Box::new(move |_| {
                        handle.block_on(async {
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
                    }),
                )
            },
            (self.offsets.rom.round_ending_entry, {
                let match_ = match_.clone();
                let handle = handle.clone();
                Box::new(move |_| {
                    handle.block_on(async {
                        let match_ = match_.lock().await;
                        let match_ = match &*match_ {
                            Some(match_) => match_,
                            None => {
                                return;
                            }
                        };

                        // This is level-triggered because otherwise it's a massive pain to deal with.
                        let mut round_state = match_.lock_round_state().await;
                        if round_state.round.is_none() {
                            return;
                        }

                        round_state.end_round().await.expect("end round");
                        match_
                            .advance_shadow_until_round_end()
                            .await
                            .expect("advance shadow");
                    });
                })
            }),
            {
                let match_ = match_.clone();
                let handle = handle.clone();
                (
                    self.offsets.rom.round_start_ret,
                    Box::new(move |_core| {
                        handle.block_on(async {
                            let match_ = match_.lock().await;
                            let match_ = match &*match_ {
                                Some(match_) => match_,
                                _ => {
                                    return;
                                }
                            };
                            match_.start_round().await.expect("start round");
                        });
                    }),
                )
            },
            {
                let match_ = match_.clone();
                let handle = handle.clone();
                (
                    self.offsets.rom.battle_is_p2_ret,
                    Box::new(move |mut core| {
                        handle.block_on(async {
                            let match_ = match_.lock().await;
                            let match_ = match &*match_ {
                                Some(match_) => match_,
                                _ => {
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
                let match_ = match_.clone();
                let handle = handle.clone();
                (
                    self.offsets.rom.link_is_p2_ret,
                    Box::new(move |mut core| {
                        handle.block_on(async {
                            let match_ = match_.lock().await;
                            let match_ = match &*match_ {
                                Some(match_) => match_,
                                _ => {
                                    return;
                                }
                            };

                            let round_state = match_.lock_round_state().await;
                            let round = match round_state.round.as_ref() {
                                Some(round) => round,
                                None => {
                                    return;
                                }
                            };

                            core.gba_mut()
                                .cpu_mut()
                                .set_gpr(0, round.local_player_index() as i32);
                        });
                    }),
                )
            },
            {
                let match_ = match_.clone();
                let munger = self.munger.clone();
                let handle = handle.clone();
                (
                    self.offsets.rom.main_read_joyflags,
                    Box::new(move |core| {
                        handle.block_on(async {
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

                            if !munger.is_linking(core) {
                                return;
                            }

                            if !round.has_committed_state() {
                                let mut rng = match_.lock_rng().await;

                                // rng2 is the shared rng, it must be synced.
                                munger.set_rng2_state(core, generate_rng2_state(&mut *rng));

                                round.set_first_committed_state(
                                    core.save_state().expect("save state"),
                                    match_
                                        .advance_shadow_until_first_committed_state()
                                        .await
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
                    }),
                )
            },
            (
                self.offsets.rom.process_battle_input_ret,
                Box::new(move |mut core| {
                    core.gba_mut().cpu_mut().set_gpr(0, 0);
                }),
            ),
            (
                self.offsets.rom.handle_input_init_send_and_receive_call,
                make_send_and_receive_call_hook(),
            ),
            (
                self.offsets.rom.handle_input_update_send_and_receive_call,
                make_send_and_receive_call_hook(),
            ),
            (
                self.offsets.rom.handle_input_deinit_send_and_receive_call,
                make_send_and_receive_call_hook(),
            ),
            {
                let munger = self.munger.clone();
                let handle = handle.clone();
                (
                    self.offsets.rom.comm_menu_send_and_receive_call,
                    Box::new(move |mut core| {
                        handle.block_on(async {
                            let pc = core.as_ref().gba().cpu().thumb_pc();
                            core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
                            core.gba_mut().cpu_mut().set_gpr(0, 3);
                            munger.set_rx_packet(core, 0, &INIT_RX);
                            munger.set_rx_packet(core, 1, &INIT_RX);
                        });
                    }),
                )
            },
            {
                (
                    self.offsets.rom.init_sio_call,
                    Box::new(move |mut core| {
                        let pc = core.as_ref().gba().cpu().thumb_pc();
                        core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
                    }),
                )
            },
            {
                let match_ = match_.clone();
                (
                    self.offsets.rom.round_call_jump_table_ret,
                    Box::new(move |_| {
                        handle.block_on(async {
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
        let make_send_and_receive_call_hook = || {
            let shadow_state = shadow_state.clone();
            let munger = self.munger.clone();

            Box::new(move |mut core: mgba::core::CoreMutRef| {
                let pc = core.as_ref().gba().cpu().thumb_pc();
                core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);

                let mut round_state = shadow_state.lock_round_state();
                let round = match round_state.round.as_mut() {
                    Some(round) => round,
                    None => {
                        core.gba_mut().cpu_mut().set_gpr(0, 0);
                        return;
                    }
                };
                core.gba_mut().cpu_mut().set_gpr(0, 3);

                let current_tick = round.current_tick();

                let ip = if let Some(ip) = round.take_shadow_input() {
                    ip
                } else {
                    let mut rx = [0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
                    byteorder::LittleEndian::write_u32(&mut rx[4..8], current_tick - 2);
                    munger.set_rx_packet(core, 0, &rx);
                    munger.set_rx_packet(core, 1, &rx);
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
                round.set_remote_packet(round.current_tick() + 1, munger.tx_packet(core).to_vec());
                round.set_input_injected();
            })
        };

        vec![
            {
                let munger = self.munger.clone();
                let shadow_state = shadow_state.clone();
                (
                    self.offsets.rom.comm_menu_init_ret,
                    Box::new(move |core| {
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
                        munger.start_battle_from_comm_menu(
                            core,
                            bn3_match_type(&mut *rng, shadow_state.match_type()),
                            random_background(&mut *rng),
                        );
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
                    self.offsets.rom.battle_is_p2_ret,
                    Box::new(move |mut core| {
                        let round_state = shadow_state.lock_round_state();
                        let round = round_state.round.as_ref().expect("round");

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
                        let round_state = shadow_state.lock_round_state();
                        let round = match round_state.round.as_ref() {
                            Some(round) => round,
                            None => {
                                return;
                            }
                        };

                        core.gba_mut()
                            .cpu_mut()
                            .set_gpr(0, round.remote_player_index() as i32);
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

                        if !munger.is_linking(core) && !round.has_first_committed_state() {
                            return;
                        }

                        if !round.has_first_committed_state() {
                            let mut rng = shadow_state.lock_rng();

                            // rng2 is the shared rng, it must be synced.
                            munger.set_rng2_state(core, generate_rng2_state(&mut *rng));

                            round.set_first_committed_state(
                                core.save_state().expect("save state"),
                                &munger.tx_packet(core),
                            );
                            log::info!(
                                "shadow rng1 state: {:08x}, rng2 state: {:08x}",
                                munger.rng1_state(core),
                                munger.rng2_state(core)
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
                            shadow_state.set_applied_state(
                                core.save_state().expect("save state"),
                                round.current_tick(),
                            );
                        }
                    }),
                )
            },
            (
                self.offsets.rom.handle_input_init_send_and_receive_call,
                make_send_and_receive_call_hook(),
            ),
            (
                self.offsets.rom.handle_input_update_send_and_receive_call,
                make_send_and_receive_call_hook(),
            ),
            (
                self.offsets.rom.handle_input_deinit_send_and_receive_call,
                make_send_and_receive_call_hook(),
            ),
            (
                self.offsets.rom.process_battle_input_ret,
                Box::new(move |mut core| {
                    core.gba_mut().cpu_mut().set_gpr(0, 0);
                }),
            ),
            {
                let munger = self.munger.clone();
                (
                    self.offsets.rom.comm_menu_send_and_receive_call,
                    Box::new(move |mut core| {
                        let pc = core.as_ref().gba().cpu().thumb_pc();
                        core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
                        core.gba_mut().cpu_mut().set_gpr(0, 3);
                        munger.set_rx_packet(core, 0, &INIT_RX);
                        munger.set_rx_packet(core, 1, &INIT_RX);
                    }),
                )
            },
            {
                (
                    self.offsets.rom.init_sio_call,
                    Box::new(move |mut core| {
                        let pc = core.as_ref().gba().cpu().thumb_pc();
                        core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
                    }),
                )
            },
            {
                let shadow_state = shadow_state.clone();
                (
                    self.offsets.rom.round_call_jump_table_ret,
                    Box::new(move |mut core| {
                        let mut round_state = shadow_state.lock_round_state();
                        let round = match round_state.round.as_mut() {
                            Some(round) => round,
                            None => {
                                return;
                            }
                        };

                        if !round.has_first_committed_state() {
                            return;
                        }
                        round.increment_current_tick();

                        if round_state.last_result.is_some() {
                            // We have no real inputs left but the round has ended. Just fudge them until we get to the next round.
                            core.gba_mut().cpu_mut().set_gpr(0, 7);
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
        let make_send_and_receive_call_hook = || {
            let munger = self.munger.clone();
            let replayer_state = replayer_state.clone();
            Box::new(move |mut core: mgba::core::CoreMutRef| {
                let mut replayer_state = replayer_state.lock_inner();

                let pc = core.as_ref().gba().cpu().thumb_pc();
                core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
                core.gba_mut().cpu_mut().set_gpr(0, 3);

                if replayer_state.is_round_ending() {
                    return;
                }

                let current_tick = replayer_state.current_tick();

                let ip = if let Some(ip) = replayer_state.pop_input_pair() {
                    ip
                } else {
                    let mut rx = [0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
                    byteorder::LittleEndian::write_u32(&mut rx[4..8], current_tick - 2);
                    munger.set_rx_packet(core, 0, &rx);
                    munger.set_rx_packet(core, 1, &rx);
                    return;
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
                replayer_state.set_local_packet(current_tick + 1, munger.tx_packet(core).to_vec());
            })
        };

        vec![
            {
                let replayer_state = replayer_state.clone();
                (
                    self.offsets.rom.battle_is_p2_ret,
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
                let replayer_state = replayer_state.clone();
                (
                    self.offsets.rom.round_ending_entry,
                    Box::new(move |_core| {
                        let mut replayer_state = replayer_state.lock_inner();
                        if replayer_state.is_round_ending() {
                            return;
                        }
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
                let replayer_state = replayer_state.clone();
                (
                    self.offsets.rom.main_read_joyflags,
                    Box::new(move |mut core| {
                        let mut replayer_state = replayer_state.lock_inner();
                        let current_tick = replayer_state.current_tick();

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
            (
                self.offsets.rom.handle_input_init_send_and_receive_call,
                make_send_and_receive_call_hook(),
            ),
            (
                self.offsets.rom.handle_input_update_send_and_receive_call,
                make_send_and_receive_call_hook(),
            ),
            (
                self.offsets.rom.handle_input_deinit_send_and_receive_call,
                make_send_and_receive_call_hook(),
            ),
            (
                self.offsets.rom.process_battle_input_ret,
                Box::new(move |mut core| {
                    core.gba_mut().cpu_mut().set_gpr(0, 0);
                }),
            ),
            {
                let replayer_state = replayer_state.clone();
                (
                    self.offsets.rom.round_call_jump_table_ret,
                    Box::new(move |_| {
                        let mut replayer_state = replayer_state.lock_inner();
                        replayer_state.increment_current_tick();
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

    fn predict_rx(&self, rx: &mut Vec<u8>) {
        let tick = byteorder::LittleEndian::read_u32(&rx[0x4..0x8]);
        byteorder::LittleEndian::write_u32(&mut rx[0x4..0x8], tick + 1);
    }

    fn prepare_for_fastforward(&self, mut core: mgba::core::CoreMutRef) {
        core.gba_mut()
            .cpu_mut()
            .set_thumb_pc(self.offsets.rom.main_read_joyflags);
    }
}
