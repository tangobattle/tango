use crate::{audio, battle, games, net, replay, replayer, stats};
use parking_lot::Mutex;
use rand::SeedableRng;
use std::sync::Arc;

pub const EXPECTED_FPS: f32 = 60.0;
pub struct Session {
    vbuf: std::sync::Arc<Mutex<Vec<u8>>>,
    _audio_binding: audio::Binding,
    thread: mgba::thread::Thread,
    joyflags: std::sync::Arc<std::sync::atomic::AtomicU32>,
    mode: Mode,
    completed: Arc<std::sync::atomic::AtomicBool>,
}

pub struct CompletionToken {
    completed: Arc<std::sync::atomic::AtomicBool>,
}

impl CompletionToken {
    pub fn complete(&self) {
        self.completed
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

pub enum Mode {
    SinglePlayer,
    PvP(std::sync::Arc<tokio::sync::Mutex<Option<std::sync::Arc<battle::Match>>>>),
    Replayer,
}

fn fix_vbuf_alpha(vbuf: &mut [u8]) {
    for i in (0..vbuf.len()).step_by(4) {
        vbuf[i + 3] = 0xff;
    }
}

#[allow(dead_code)] // TODO
impl Session {
    pub fn new_pvp(
        handle: tokio::runtime::Handle,
        audio_binder: audio::LateBinder,
        link_code: String,
        local_settings: net::protocol::Settings,
        local_game: &'static (dyn games::Game + Send + Sync),
        local_rom: &[u8],
        local_save: &[u8],
        remote_settings: net::protocol::Settings,
        remote_game: &'static (dyn games::Game + Send + Sync),
        remote_rom: &[u8],
        remote_save: &[u8],
        emu_tps_counter: Arc<Mutex<stats::Counter>>,
        sender: net::Sender,
        receiver: net::Receiver,
        is_offerer: bool,
        replays_path: std::path::PathBuf,
        match_type: (u8, u8),
        input_delay: u32,
        rng_seed: [u8; 16],
        max_queue_length: usize,
    ) -> Result<Self, anyhow::Error> {
        let mut core = mgba::core::Core::new_gba("tango")?;
        core.enable_video_buffer();

        core.as_mut()
            .load_rom(mgba::vfile::VFile::open_memory(&local_rom))?;
        core.as_mut()
            .load_save(mgba::vfile::VFile::open_memory(&local_save))?;

        let joyflags = Arc::new(std::sync::atomic::AtomicU32::new(0));

        let game = games::find_by_rom_info(&core.as_mut().rom_code(), core.as_mut().rom_revision())
            .unwrap();
        let hooks = game.hooks();
        hooks.patch(core.as_mut());

        let completed = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let match_ = std::sync::Arc::new(tokio::sync::Mutex::new(None));
        let _ = std::fs::create_dir_all(replays_path.parent().unwrap());
        let mut traps = hooks.common_traps();
        traps.extend(hooks.primary_traps(
            handle.clone(),
            joyflags.clone(),
            match_.clone(),
            CompletionToken {
                completed: completed.clone(),
            },
        ));
        core.set_traps(traps);

        let thread = mgba::thread::Thread::new(core);

        let match_ = match_.clone();
        *match_.try_lock().unwrap() = Some({
            let inner_match = battle::Match::new(
                link_code,
                local_rom.to_vec(),
                local_game,
                local_settings,
                remote_game,
                remote_settings,
                sender,
                rand_pcg::Mcg128Xsl64::from_seed(rng_seed),
                is_offerer,
                thread.handle(),
                remote_rom,
                remote_save,
                replays_path,
                match_type,
                input_delay,
                max_queue_length,
            )
            .expect("new match");

            {
                let match_ = match_.clone();
                let inner_match = inner_match.clone();
                handle.spawn(async move {
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
        thread
            .handle()
            .lock_audio()
            .sync_mut()
            .set_fps_target(EXPECTED_FPS);

        let audio_binding = audio_binder.bind(Some(Box::new(audio::MGBAStream::new(
            thread.handle(),
            audio_binder.supported_config().sample_rate(),
        ))))?;

        let vbuf = Arc::new(Mutex::new(vec![
            0u8;
            (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT * 4)
                as usize
        ]));
        {
            let joyflags = joyflags.clone();
            let vbuf = vbuf.clone();
            let emu_tps_counter = emu_tps_counter.clone();
            thread.set_frame_callback(move |mut core, video_buffer| {
                let mut vbuf = vbuf.lock();
                vbuf.copy_from_slice(video_buffer);
                fix_vbuf_alpha(&mut vbuf);
                core.set_keys(joyflags.load(std::sync::atomic::Ordering::Relaxed));
                emu_tps_counter.lock().mark();
            });
        }
        Ok(Session {
            vbuf,
            _audio_binding: audio_binding,
            thread,
            joyflags,
            mode: Mode::PvP(match_),
            completed,
        })
    }

    pub fn new_singleplayer(
        audio_binder: audio::LateBinder,
        rom: &[u8],
        save_path: &std::path::Path,
        emu_tps_counter: Arc<Mutex<stats::Counter>>,
    ) -> Result<Self, anyhow::Error> {
        let mut core = mgba::core::Core::new_gba("tango")?;
        core.enable_video_buffer();

        core.as_mut()
            .load_rom(mgba::vfile::VFile::open_memory(rom))?;

        let save_vf = mgba::vfile::VFile::open(
            save_path,
            mgba::vfile::flags::O_CREAT | mgba::vfile::flags::O_RDWR,
        )?;

        core.as_mut().load_save(save_vf)?;

        let joyflags = Arc::new(std::sync::atomic::AtomicU32::new(0));

        let game = games::find_by_rom_info(&core.as_mut().rom_code(), core.as_mut().rom_revision())
            .unwrap();
        let hooks = game.hooks();
        hooks.patch(core.as_mut());

        let completed = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let thread = mgba::thread::Thread::new(core);

        thread.start()?;
        thread
            .handle()
            .lock_audio()
            .sync_mut()
            .set_fps_target(EXPECTED_FPS);

        let audio_binding = audio_binder.bind(Some(Box::new(audio::MGBAStream::new(
            thread.handle(),
            audio_binder.supported_config().sample_rate(),
        ))))?;

        let vbuf = Arc::new(Mutex::new(vec![
            0u8;
            (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT * 4)
                as usize
        ]));
        {
            let joyflags = joyflags.clone();
            let vbuf = vbuf.clone();
            let emu_tps_counter = emu_tps_counter.clone();
            thread.set_frame_callback(move |mut core, video_buffer| {
                let mut vbuf = vbuf.lock();
                vbuf.copy_from_slice(video_buffer);
                fix_vbuf_alpha(&mut vbuf);
                core.set_keys(joyflags.load(std::sync::atomic::Ordering::Relaxed));
                emu_tps_counter.lock().mark();
            });
        }
        Ok(Session {
            vbuf,
            _audio_binding: audio_binding,
            thread,
            joyflags,
            mode: Mode::SinglePlayer,
            completed,
        })
    }

    pub fn new_replayer(
        audio_binder: audio::LateBinder,
        rom: &[u8],
        emu_tps_counter: Arc<Mutex<stats::Counter>>,
        replay: replay::Replay,
    ) -> Result<Self, anyhow::Error> {
        let mut core = mgba::core::Core::new_gba("tango")?;
        core.enable_video_buffer();

        core.as_mut()
            .load_rom(mgba::vfile::VFile::open_memory(&rom))?;

        let game = games::find_by_rom_info(&core.as_mut().rom_code(), core.as_mut().rom_revision())
            .unwrap();
        let hooks = game.hooks();
        hooks.patch(core.as_mut());

        let completed = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let input_pairs = replay.input_pairs.clone();
        let replayer_state = replayer::State::new(
            replay.local_player_index,
            input_pairs,
            0,
            Box::new({
                let completed = completed.clone();
                move || {
                    // TODO: This probably crashes maybe?
                    completed.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }),
        );
        let mut traps = hooks.common_traps();
        traps.extend(hooks.replayer_traps(replayer_state.clone()));
        core.set_traps(traps);

        let thread = mgba::thread::Thread::new(core);

        thread.start()?;
        thread.handle().pause();
        thread
            .handle()
            .lock_audio()
            .sync_mut()
            .set_fps_target(EXPECTED_FPS);

        let audio_binding = audio_binder.bind(Some(Box::new(audio::MGBAStream::new(
            thread.handle(),
            audio_binder.supported_config().sample_rate(),
        ))))?;

        thread.handle().run_on_core(move |mut core| {
            core.load_state(replay.local_state.as_ref().unwrap())
                .expect("load state");
        });
        thread.handle().unpause();

        let vbuf = Arc::new(Mutex::new(vec![
            0u8;
            (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT * 4)
                as usize
        ]));
        {
            let vbuf = vbuf.clone();
            let emu_tps_counter = emu_tps_counter.clone();
            thread.set_frame_callback(move |_core, video_buffer| {
                let mut vbuf = vbuf.lock();
                vbuf.copy_from_slice(video_buffer);
                fix_vbuf_alpha(&mut vbuf);
                emu_tps_counter.lock().mark();
            });
        }

        Ok(Session {
            vbuf,
            _audio_binding: audio_binding,
            thread,
            joyflags: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            mode: Mode::Replayer,
            completed,
        })
    }

    pub fn completed(&self) -> bool {
        self.completed.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn mode(&self) -> &Mode {
        &self.mode
    }

    pub fn set_fps(&self, fps: f32) {
        let handle = self.thread.handle();
        let audio_guard = handle.lock_audio();
        audio_guard.sync_mut().set_fps_target(fps);
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

    pub fn set_joyflags(&self, joyflags: u32) {
        self.joyflags
            .store(joyflags, std::sync::atomic::Ordering::Relaxed);
    }
}
