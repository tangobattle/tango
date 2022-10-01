use crate::{audio, battle, config, game, net, replay, replayer, rom, save, stats, video};
use parking_lot::Mutex;
use rand::SeedableRng;
use std::sync::Arc;

pub const EXPECTED_FPS: f32 = 60.0;

pub struct GameInfo {
    pub game: &'static (dyn game::Game + Send + Sync),
    pub patch: Option<(String, semver::Version)>,
}

pub struct Setup {
    pub game_lang: unic_langid::LanguageIdentifier,
    pub save: Box<dyn save::Save + Send + Sync>,
    pub assets: Box<dyn rom::Assets + Send + Sync>,
}

pub struct Session {
    start_time: std::time::SystemTime,
    game_info: GameInfo,
    vbuf: std::sync::Arc<Mutex<Vec<u8>>>,
    _audio_binding: audio::Binding,
    thread: mgba::thread::Thread,
    joyflags: std::sync::Arc<std::sync::atomic::AtomicU32>,
    mode: Mode,
    completion_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pause_on_next_frame: std::sync::Arc<std::sync::atomic::AtomicBool>,
    opponent_setup: Option<Setup>,
    own_setup: Option<Setup>,
}

pub struct CompletionToken {
    flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl CompletionToken {
    pub fn complete(&self) {
        self.flag.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

pub struct PvP {
    pub match_: std::sync::Arc<tokio::sync::Mutex<Option<std::sync::Arc<battle::Match>>>>,
    cancellation_token: tokio_util::sync::CancellationToken,
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
        local_save: &[u8],
        remote_settings: net::protocol::Settings,
        remote_game: &'static (dyn game::Game + Send + Sync),
        remote_patch_overrides: &rom::Overrides,
        remote_rom: &[u8],
        remote_save: &[u8],
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

        core.as_mut().load_rom(mgba::vfile::VFile::open_memory(&local_rom))?;
        core.as_mut().load_save(mgba::vfile::VFile::open_memory(&local_save))?;

        let joyflags = Arc::new(std::sync::atomic::AtomicU32::new(0));

        let game = game::find_by_rom_info(&core.as_mut().rom_code(), core.as_mut().rom_revision()).unwrap();
        let hooks = game.hooks();
        hooks.patch(core.as_mut());

        let match_ = std::sync::Arc::new(tokio::sync::Mutex::new(None));
        let _ = std::fs::create_dir_all(replays_path.parent().unwrap());
        let mut traps = hooks.common_traps();

        let completion_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

        traps.extend(hooks.primary_traps(
            joyflags.clone(),
            match_.clone(),
            CompletionToken {
                flag: completion_flag.clone(),
            },
        ));
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

        let cancellation_token = tokio_util::sync::CancellationToken::new();
        let match_ = match_.clone();
        *match_.try_lock().unwrap() = Some({
            let inner_match = battle::Match::new(
                config,
                link_code,
                netplay_compatibility,
                local_rom.to_vec(),
                local_game,
                local_settings,
                remote_settings,
                cancellation_token.clone(),
                sender,
                peer_conn,
                rand_pcg::Mcg128Xsl64::from_seed(rng_seed),
                is_offerer,
                thread.handle(),
                remote_rom,
                remote_save,
                replays_path,
                match_type,
            )
            .expect("new match");

            {
                let match_ = match_.clone();
                let inner_match = inner_match.clone();
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
            let completion_flag = completion_flag.clone();
            let joyflags = joyflags.clone();
            let vbuf = vbuf.clone();
            let emu_tps_counter = emu_tps_counter.clone();
            move |mut core, video_buffer, mut thread_handle| {
                let mut vbuf = vbuf.lock();
                vbuf.copy_from_slice(video_buffer);
                video::fix_vbuf_alpha(&mut *vbuf);
                core.set_keys(joyflags.load(std::sync::atomic::Ordering::Relaxed));
                emu_tps_counter.lock().mark();

                if completion_flag.load(std::sync::atomic::Ordering::SeqCst) {
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
            }),
            completion_flag,
            pause_on_next_frame: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            own_setup: {
                let save = local_game.parse_save(&local_save)?;
                let assets = local_game.load_rom_assets(&local_rom, save.as_raw_wram(), local_patch_overrides)?;
                Some(Setup {
                    game_lang: local_patch_overrides
                        .language
                        .clone()
                        .unwrap_or_else(|| game.language()),
                    save,
                    assets,
                })
            },
            opponent_setup: if reveal_setup {
                let save = remote_game.parse_save(&remote_save)?;
                let assets = remote_game.load_rom_assets(&remote_rom, save.as_raw_wram(), remote_patch_overrides)?;
                Some(Setup {
                    game_lang: remote_patch_overrides
                        .language
                        .clone()
                        .unwrap_or_else(|| game.language()),
                    save,
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
        save_path: &std::path::Path,
        emu_tps_counter: Arc<Mutex<stats::Counter>>,
    ) -> Result<Self, anyhow::Error> {
        let mut core = mgba::core::Core::new_gba("tango")?;
        core.enable_video_buffer();

        core.as_mut().load_rom(mgba::vfile::VFile::open_memory(rom))?;

        let save_vf = mgba::vfile::VFile::open(save_path, mgba::vfile::flags::O_CREAT | mgba::vfile::flags::O_RDWR)?;

        core.as_mut().load_save(save_vf)?;

        let joyflags = Arc::new(std::sync::atomic::AtomicU32::new(0));

        let hooks = game.hooks();
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
            completion_flag: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
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
        replay: &replay::Replay,
    ) -> Result<Self, anyhow::Error> {
        let mut core = mgba::core::Core::new_gba("tango")?;
        core.enable_video_buffer();

        core.as_mut().load_rom(mgba::vfile::VFile::open_memory(&rom))?;

        let hooks = game.hooks();
        hooks.patch(core.as_mut());

        let completion_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

        let replay_is_complete = replay.is_complete;
        let input_pairs = replay.input_pairs.clone();
        let replayer_state = replayer::State::new(
            (replay.metadata.match_type as u8, replay.metadata.match_subtype as u8),
            replay.local_player_index,
            input_pairs,
            0,
            Box::new({
                let completion_flag = completion_flag.clone();
                move || {
                    completion_flag.store(true, std::sync::atomic::Ordering::SeqCst);
                }
            }),
        );
        let mut traps = hooks.common_traps();
        traps.extend(hooks.replayer_traps(replayer_state.clone()));
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
            core.load_state(local_state.as_ref().unwrap()).expect("load state");
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
            let completion_flag = completion_flag.clone();
            let replayer_state = replayer_state.clone();
            let pause_on_next_frame = pause_on_next_frame.clone();
            move |_core, video_buffer, mut thread_handle| {
                let mut vbuf = vbuf.lock();
                vbuf.copy_from_slice(video_buffer);
                video::fix_vbuf_alpha(&mut *vbuf);
                emu_tps_counter.lock().mark();

                if !replay_is_complete && replayer_state.lock_inner().input_pairs_left() == 0 {
                    completion_flag.store(true, std::sync::atomic::Ordering::SeqCst);
                }

                if pause_on_next_frame.swap(false, std::sync::atomic::Ordering::SeqCst)
                    || completion_flag.load(std::sync::atomic::Ordering::SeqCst)
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
            completion_flag,
            pause_on_next_frame,
            own_setup: None,
            opponent_setup: None,
        })
    }

    pub fn completed(&self) -> bool {
        self.completion_flag.load(std::sync::atomic::Ordering::SeqCst)
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
