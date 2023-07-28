use crate::{audio, config, game, net, rom, stats, video};
use parking_lot::Mutex;
use rand::SeedableRng;
use std::sync::Arc;

pub const EXPECTED_FPS: f32 = 16777216.0 / 280896.0;

pub struct GameInfo {
    pub game: &'static (dyn game::Game + Send + Sync),
    pub patch: Option<(String, semver::Version)>,
}

pub struct Setup {
    pub game_lang: unic_langid::LanguageIdentifier,
    pub save: Box<dyn tango_dataview::save::Save + Send + Sync>,
    pub assets: Box<dyn tango_dataview::rom::Assets + Send + Sync>,
}

pub struct Session {
    start_time: std::time::SystemTime,
    game_info: GameInfo,
    vbuf: std::sync::Arc<Mutex<Vec<u8>>>,
    _audio_binding: audio::Binding,
    thread: mgba::thread::Thread,
    joyflags: std::sync::Arc<std::sync::atomic::AtomicU32>,
    mode: Mode,
    completion_token: tango_pvp::hooks::CompletionToken,
    pause_on_next_frame: std::sync::Arc<std::sync::atomic::AtomicBool>,
    opponent_setup: Option<Setup>,
    own_setup: Option<Setup>,
}

pub struct PvP {
    pub match_: std::sync::Arc<tokio::sync::Mutex<Option<std::sync::Arc<tango_pvp::battle::Match>>>>,
    cancellation_token: tokio_util::sync::CancellationToken,
    latency_counter: std::sync::Arc<tokio::sync::Mutex<crate::stats::LatencyCounter>>,
    _peer_conn: datachannel_wrapper::PeerConnection,
}

impl PvP {
    pub async fn latency(&self) -> std::time::Duration {
        self.latency_counter.lock().await.median()
    }
}

pub struct SinglePlayer {}

pub enum Mode {
    SinglePlayer(SinglePlayer),
    PvP(PvP),
    Replayer,
}

impl Session {
    pub fn new_pvp(
        config: std::sync::Arc<parking_lot::RwLock<config::Config>>,
        audio_binder: audio::LateBinder,
        link_code: String,
        netplay_compatibility: String,
        local_settings: net::protocol::Settings,
        local_game: &'static (dyn game::Game + Send + Sync),
        local_patch: Option<(String, semver::Version)>,
        local_patch_overrides: &rom::Overrides,
        local_rom: &[u8],
        local_save: Box<dyn tango_dataview::save::Save + Send + Sync + 'static>,
        remote_settings: net::protocol::Settings,
        remote_game: &'static (dyn game::Game + Send + Sync),
        remote_patch_overrides: &rom::Overrides,
        remote_rom: &[u8],
        remote_save: Box<dyn tango_dataview::save::Save + Send + Sync + 'static>,
        emu_tps_counter: Arc<Mutex<stats::Counter>>,
        sender: net::Sender,
        receiver: net::Receiver,
        peer_conn: datachannel_wrapper::PeerConnection,
        is_offerer: bool,
        replays_path: std::path::PathBuf,
        match_type: (u8, u8),
        rng_seed: [u8; 16],
    ) -> Result<Self, anyhow::Error> {
        let mut core = mgba::core::Core::new_gba("tango")?;
        core.enable_video_buffer();

        core.as_mut()
            .load_rom(mgba::vfile::VFile::from_vec(local_rom.to_vec()))?;
        core.as_mut()
            .load_save(mgba::vfile::VFile::from_vec(local_save.as_sram_dump()))?;

        let joyflags = Arc::new(std::sync::atomic::AtomicU32::new(0));

        let local_hooks = tango_pvp::hooks::hooks_for_gamedb_entry(local_game.gamedb_entry()).unwrap();
        local_hooks.patch(core.as_mut());

        let match_ = std::sync::Arc::new(tokio::sync::Mutex::new(None));
        let _ = std::fs::create_dir_all(replays_path.parent().unwrap());
        let mut traps = local_hooks.common_traps();

        let completion_token = tango_pvp::hooks::CompletionToken::new();

        traps.extend(local_hooks.primary_traps(joyflags.clone(), match_.clone(), completion_token.clone()));
        core.set_traps(
            traps
                .into_iter()
                .map(|(addr, f)| {
                    let handle = tokio::runtime::Handle::current();
                    (
                        addr,
                        Box::new(move |core: mgba::core::CoreMutRef<'_>| {
                            let _guard = handle.enter();
                            f(core)
                        }) as Box<dyn Fn(mgba::core::CoreMutRef<'_>)>,
                    )
                })
                .collect(),
        );

        let reveal_setup = remote_settings.reveal_setup;

        let thread = mgba::thread::Thread::new(core);

        let sender = std::sync::Arc::new(tokio::sync::Mutex::new(sender));
        let latency_counter = std::sync::Arc::new(tokio::sync::Mutex::new(crate::stats::LatencyCounter::new(5)));

        let cancellation_token = tokio_util::sync::CancellationToken::new();
        let match_ = match_.clone();
        *match_.try_lock().unwrap() = Some({
            let config = config.read();
            let replays_path = config.replays_path();
            let link_code = link_code.clone();
            let netplay_compatibility = netplay_compatibility.clone();
            let local_settings = local_settings.clone();
            let remote_settings = remote_settings.clone();
            let replaycollector_endpoint = config.replaycollector_endpoint.clone();
            let inner_match = tango_pvp::battle::Match::new(
                local_rom.to_vec(),
                local_hooks,
                tango_pvp::hooks::hooks_for_gamedb_entry(remote_game.gamedb_entry()).unwrap(),
                cancellation_token.clone(),
                Box::new(crate::net::PvpSender::new(sender.clone())),
                rand_pcg::Mcg128Xsl64::from_seed(rng_seed),
                is_offerer,
                thread.handle(),
                remote_rom,
                remote_save.as_ref(),
                match_type,
                config.input_delay,
                move |round_number, local_player_index| {
                    const TIME_DESCRIPTION: &[time::format_description::FormatItem<'_>] = time::macros::format_description!(
                        "[year padding:zero][month padding:zero repr:numerical][day padding:zero][hour padding:zero][minute padding:zero][second padding:zero]"
                    );
                    let replay_filename = replays_path.join(format!(
                        "{}.tangoreplay",
                        format!(
                            "{}-{}-{}-vs-{}-round{}-p{}",
                            time::OffsetDateTime::from(std::time::SystemTime::now())
                                .format(TIME_DESCRIPTION)
                                .expect("format time"),
                            link_code,
                            netplay_compatibility,
                            remote_settings.nickname,
                            round_number,
                            local_player_index + 1
                        )
                        .chars()
                        .filter(|c| "/\\?%*:|\"<>. ".chars().all(|c2| c2 != *c))
                        .collect::<String>()
                    ));
                    log::info!("open replay: {}", replay_filename.display());

                    let local_game_settings = local_settings.game_info.as_ref().unwrap();
                    let remote_game_settings = remote_settings.game_info.as_ref().unwrap();

                    let replay_file = std::fs::OpenOptions::new().read(true).write(true).create(true).open(&replay_filename)?;
                    Ok(Some(tango_pvp::replay::Writer::new(
                        replay_file,
                        tango_pvp::replay::Metadata {
                            ts: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_millis() as u64,
                            link_code: link_code.clone(),
                            local_side: Some(tango_pvp::replay::metadata::Side {
                                nickname: local_settings.nickname.clone(),
                                game_info: Some(tango_pvp::replay::metadata::GameInfo {
                                    rom_family: local_game_settings.family_and_variant.0.to_string(),
                                    rom_variant: local_game_settings.family_and_variant.1 as u32,
                                    patch: if let Some(patch) = local_game_settings.patch.as_ref() {
                                        Some(tango_pvp::replay::metadata::game_info::Patch {
                                            name: patch.name.clone(),
                                            version: patch.version.to_string(),
                                        })
                                    } else {
                                        None
                                    },
                                }),
                                reveal_setup: local_settings.reveal_setup,
                            }),
                            remote_side: Some(tango_pvp::replay::metadata::Side {
                                nickname: remote_settings.nickname.clone(),
                                game_info: Some(tango_pvp::replay::metadata::GameInfo {
                                    rom_family: remote_game_settings.family_and_variant.0.to_string(),
                                    rom_variant: remote_game_settings.family_and_variant.1 as u32,
                                    patch: if let Some(patch) = remote_game_settings.patch.as_ref() {
                                        Some(tango_pvp::replay::metadata::game_info::Patch {
                                            name: patch.name.clone(),
                                            version: patch.version.to_string(),
                                        })
                                    } else {
                                        None
                                    },
                                }),
                                reveal_setup: remote_settings.reveal_setup,
                            }),
                            round: round_number as u32,
                            match_type: match_type.0 as u32,
                            match_subtype: match_type.1 as u32,
                        },
                        local_player_index,
                        local_hooks.packet_size() as u8,
                    )?))
                },
                move |r| {
                    if replaycollector_endpoint.is_empty() {
                        return Ok(());
                    }

                    let mut buf = vec![];
                    r.read_to_end(&mut buf)?;

                    let replaycollector_endpoint = replaycollector_endpoint.clone();

                    tokio::spawn(async move {
                        if let Err(e) = (move || async move {
                            let client = reqwest::Client::new();
                            client
                                .post(replaycollector_endpoint)
                                .header("Content-Type", "application/x-tango-replay")
                                .body(buf)
                                .send()
                                .await?
                                .error_for_status()?;
                            Ok::<(), anyhow::Error>(())
                        })()
                        .await
                        {
                            log::error!("failed to submit replay: {:?}", e);
                        }
                    });

                    Ok(())
                },
            )
            .expect("new match");

            {
                let match_ = match_.clone();
                let inner_match = inner_match.clone();
                let receiver = Box::new(crate::net::PvpReceiver::new(
                    receiver,
                    sender.clone(),
                    latency_counter.clone(),
                ));
                tokio::task::spawn(async move {
                    tokio::select! {
                        r = inner_match.run(receiver) => {
                            log::info!("match thread ending: {:?}", r);
                        }
                        _ = inner_match.cancelled() => {
                        }
                    }
                    log::info!("match thread ended");
                    *match_.lock().await = None;
                });
            }

            inner_match
        });

        thread.start()?;
        thread.handle().lock_audio().sync_mut().set_fps_target(EXPECTED_FPS);

        let audio_binding = audio_binder.bind(Some(Box::new(audio::MGBAStream::new(
            thread.handle(),
            audio_binder.sample_rate(),
        ))))?;

        let vbuf = Arc::new(Mutex::new(vec![
            0u8;
            (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT * 4)
                as usize
        ]));
        thread.set_frame_callback({
            let completion_token = completion_token.clone();
            let joyflags = joyflags.clone();
            let vbuf = vbuf.clone();
            let emu_tps_counter = emu_tps_counter.clone();
            move |mut core, video_buffer, mut thread_handle| {
                let mut vbuf = vbuf.lock();
                vbuf.copy_from_slice(video_buffer);
                video::fix_vbuf_alpha(&mut *vbuf);
                core.set_keys(joyflags.load(std::sync::atomic::Ordering::Relaxed));
                emu_tps_counter.lock().mark();

                if completion_token.is_complete() {
                    thread_handle.pause();
                }
            }
        });

        Ok(Session {
            start_time: std::time::SystemTime::now(),
            game_info: GameInfo {
                game: local_game,
                patch: local_patch,
            },
            vbuf,
            _audio_binding: audio_binding,
            thread,
            joyflags,
            mode: Mode::PvP(PvP {
                match_,
                cancellation_token,
                _peer_conn: peer_conn,
                latency_counter,
            }),
            completion_token,
            pause_on_next_frame: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            own_setup: {
                let assets =
                    local_game.load_rom_assets(&local_rom, &local_save.as_raw_wram(), local_patch_overrides)?;
                Some(Setup {
                    game_lang: local_patch_overrides
                        .language
                        .clone()
                        .unwrap_or_else(|| crate::game::region_to_language(local_game.gamedb_entry().region)),
                    save: local_save,
                    assets,
                })
            },
            opponent_setup: if reveal_setup {
                let assets =
                    remote_game.load_rom_assets(&remote_rom, &remote_save.as_raw_wram(), remote_patch_overrides)?;
                Some(Setup {
                    game_lang: remote_patch_overrides
                        .language
                        .clone()
                        .unwrap_or_else(|| crate::game::region_to_language(remote_game.gamedb_entry().region)),
                    save: remote_save,
                    assets,
                })
            } else {
                None
            },
        })
    }

    pub fn new_singleplayer(
        audio_binder: audio::LateBinder,
        game: &'static (dyn game::Game + Send + Sync),
        patch: Option<(String, semver::Version)>,
        rom: &[u8],
        save_file: std::fs::File,
        emu_tps_counter: Arc<Mutex<stats::Counter>>,
    ) -> Result<Self, anyhow::Error> {
        let mut core = mgba::core::Core::new_gba("tango")?;
        core.enable_video_buffer();

        core.as_mut().load_rom(mgba::vfile::VFile::from_vec(rom.to_vec()))?;

        let save_vf = mgba::vfile::VFile::from_file(save_file);

        core.as_mut().load_save(save_vf)?;

        let joyflags = Arc::new(std::sync::atomic::AtomicU32::new(0));

        let hooks = tango_pvp::hooks::hooks_for_gamedb_entry(game.gamedb_entry()).unwrap();
        hooks.patch(core.as_mut());

        let thread = mgba::thread::Thread::new(core);

        thread.start()?;
        thread.handle().lock_audio().sync_mut().set_fps_target(EXPECTED_FPS);

        let audio_binding = audio_binder.bind(Some(Box::new(audio::MGBAStream::new(
            thread.handle(),
            audio_binder.sample_rate(),
        ))))?;

        let pause_on_next_frame = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let vbuf = Arc::new(Mutex::new(vec![
            0u8;
            (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT * 4)
                as usize
        ]));
        thread.set_frame_callback({
            let joyflags = joyflags.clone();
            let vbuf = vbuf.clone();
            let emu_tps_counter = emu_tps_counter.clone();
            let pause_on_next_frame = pause_on_next_frame.clone();
            move |mut core, video_buffer, mut thread_handle| {
                let mut vbuf = vbuf.lock();
                vbuf.copy_from_slice(video_buffer);
                video::fix_vbuf_alpha(&mut *vbuf);
                core.set_keys(joyflags.load(std::sync::atomic::Ordering::Relaxed));
                emu_tps_counter.lock().mark();

                if pause_on_next_frame.swap(false, std::sync::atomic::Ordering::SeqCst) {
                    thread_handle.pause();
                }
            }
        });
        Ok(Session {
            start_time: std::time::SystemTime::now(),
            game_info: GameInfo { game, patch },
            vbuf,
            _audio_binding: audio_binding,
            thread,
            joyflags,
            mode: Mode::SinglePlayer(SinglePlayer {}),
            pause_on_next_frame,
            completion_token: tango_pvp::hooks::CompletionToken::new(),
            own_setup: None,
            opponent_setup: None,
        })
    }

    pub fn new_replayer(
        audio_binder: audio::LateBinder,
        game: &'static (dyn game::Game + Send + Sync),
        patch: Option<(String, semver::Version)>,
        rom: &[u8],
        emu_tps_counter: Arc<Mutex<stats::Counter>>,
        replay: &tango_pvp::replay::Replay,
    ) -> Result<Self, anyhow::Error> {
        let mut core = mgba::core::Core::new_gba("tango")?;
        core.enable_video_buffer();

        core.as_mut().load_rom(mgba::vfile::VFile::from_vec(rom.to_vec()))?;

        let hooks = tango_pvp::hooks::hooks_for_gamedb_entry(game.gamedb_entry()).unwrap();
        hooks.patch(core.as_mut());

        let completion_token = tango_pvp::hooks::CompletionToken::new();

        let replay_is_complete = replay.is_complete;
        let input_pairs = replay.input_pairs.clone();
        let stepper_state = tango_pvp::stepper::State::new(
            (replay.metadata.match_type as u8, replay.metadata.match_subtype as u8),
            replay.local_player_index,
            input_pairs.iter().map(|p| p.clone().into()).collect(),
            0,
            Box::new({
                let completion_token = completion_token.clone();
                move || {
                    completion_token.complete();
                }
            }),
        );
        let mut traps = hooks.common_traps();
        traps.extend(hooks.stepper_traps(stepper_state.clone()));
        traps.extend(hooks.stepper_replay_traps());
        core.set_traps(traps);

        let thread = mgba::thread::Thread::new(core);

        thread.start()?;
        thread.handle().pause();
        thread.handle().lock_audio().sync_mut().set_fps_target(EXPECTED_FPS);

        let audio_binding = audio_binder.bind(Some(Box::new(audio::MGBAStream::new(
            thread.handle(),
            audio_binder.sample_rate(),
        ))))?;

        let local_state = replay.local_state.clone();
        thread.handle().run_on_core(move |mut core| {
            core.load_state(&local_state).expect("load state");
        });
        thread.handle().unpause();

        let pause_on_next_frame = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let vbuf = Arc::new(Mutex::new(vec![
            0u8;
            (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT * 4)
                as usize
        ]));
        thread.set_frame_callback({
            let vbuf = vbuf.clone();
            let emu_tps_counter = emu_tps_counter.clone();
            let completion_token = completion_token.clone();
            let stepper_state = stepper_state.clone();
            let pause_on_next_frame = pause_on_next_frame.clone();
            move |_core, video_buffer, mut thread_handle| {
                let mut vbuf = vbuf.lock();
                vbuf.copy_from_slice(video_buffer);
                video::fix_vbuf_alpha(&mut *vbuf);
                emu_tps_counter.lock().mark();

                if !replay_is_complete && stepper_state.lock_inner().input_pairs_left() == 0 {
                    completion_token.complete();
                }

                if pause_on_next_frame.swap(false, std::sync::atomic::Ordering::SeqCst)
                    || completion_token.is_complete()
                {
                    thread_handle.pause();
                }
            }
        });

        Ok(Session {
            start_time: std::time::SystemTime::now(),
            game_info: GameInfo { game, patch },
            vbuf,
            _audio_binding: audio_binding,
            thread,
            joyflags: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            mode: Mode::Replayer,
            completion_token,
            pause_on_next_frame,
            own_setup: None,
            opponent_setup: None,
        })
    }

    pub fn completed(&self) -> bool {
        self.completion_token.is_complete()
    }

    pub fn mode(&self) -> &Mode {
        &self.mode
    }

    pub fn set_paused(&self, pause: bool) {
        let handle = self.thread.handle();
        if pause {
            handle.pause();
        } else {
            handle.unpause();
        }
    }

    pub fn is_paused(&self) -> bool {
        let handle = self.thread.handle();
        handle.is_paused()
    }

    pub fn frame_step(&self) {
        self.pause_on_next_frame
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let handle = self.thread.handle();
        handle.unpause();
    }

    pub fn set_fps_target(&self, fps: f32) {
        let handle = self.thread.handle();
        let audio_guard = handle.lock_audio();
        audio_guard.sync_mut().set_fps_target(fps);
    }

    pub fn fps_target(&self) -> f32 {
        let handle = self.thread.handle();
        let audio_guard = handle.lock_audio();
        audio_guard.sync().fps_target()
    }

    pub fn set_master_volume(&self, volume: i32) {
        let handle = self.thread.handle();
        let mut audio_guard = handle.lock_audio();
        audio_guard.core_mut().gba_mut().set_master_volume(volume);
    }

    pub fn has_crashed(&self) -> Option<mgba::thread::Handle> {
        let handle = self.thread.handle();
        if handle.has_crashed() {
            Some(handle)
        } else {
            None
        }
    }

    pub fn lock_vbuf(&self) -> parking_lot::MutexGuard<Vec<u8>> {
        self.vbuf.lock()
    }

    pub fn thread_handle(&self) -> mgba::thread::Handle {
        self.thread.handle()
    }

    pub fn set_joyflags(&self, joyflags: u32) {
        self.joyflags.store(joyflags, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn game_info(&self) -> &GameInfo {
        &self.game_info
    }

    pub fn start_time(&self) -> std::time::SystemTime {
        self.start_time
    }

    pub fn opponent_setup(&self) -> &Option<Setup> {
        &self.opponent_setup
    }

    pub fn own_setup(&self) -> &Option<Setup> {
        &self.own_setup
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        match &mut self.mode {
            Mode::PvP(pvp) => {
                pvp.cancellation_token.cancel();
            }
            _ => {}
        }
    }
}
