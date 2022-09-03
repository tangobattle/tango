use crate::{audio, battle, config, game, net, replay, replayer, stats};
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
    completion_rx: oneshot::Receiver<()>,
}

pub struct CompletionToken {
    tx: std::sync::Arc<parking_lot::Mutex<Option<oneshot::Sender<()>>>>,
}

impl CompletionToken {
    pub fn complete(&self) {
        if let Some(tx) = self.tx.lock().take() {
            let _ = tx.send(());
        }
    }
}

pub struct PvP {
    pub match_: std::sync::Arc<tokio::sync::Mutex<Option<std::sync::Arc<battle::Match>>>>,
    cancellation_token: tokio_util::sync::CancellationToken,
}

pub struct SinglePlayer {
    completion_tx: oneshot::Sender<()>,
}

pub enum Mode {
    SinglePlayer(SinglePlayer),
    PvP(PvP),
    Replayer,
}

#[allow(dead_code)] // TODO
impl Session {
    pub fn new_pvp(
        config: std::sync::Arc<parking_lot::RwLock<config::Config>>,
        handle: tokio::runtime::Handle,
        audio_binder: audio::LateBinder,
        link_code: String,
        netplay_compatibility: String,
        local_settings: net::protocol::Settings,
        local_game: &'static (dyn game::Game + Send + Sync),
        local_rom: &[u8],
        local_save: &[u8],
        remote_settings: net::protocol::Settings,
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

        core.as_mut()
            .load_rom(mgba::vfile::VFile::open_memory(&local_rom))?;
        core.as_mut()
            .load_save(mgba::vfile::VFile::open_memory(&local_save))?;

        let joyflags = Arc::new(std::sync::atomic::AtomicU32::new(0));

        let game = game::find_by_rom_info(&core.as_mut().rom_code(), core.as_mut().rom_revision())
            .unwrap();
        let hooks = game.hooks();
        hooks.patch(core.as_mut());

        let completed = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let match_ = std::sync::Arc::new(tokio::sync::Mutex::new(None));
        let _ = std::fs::create_dir_all(replays_path.parent().unwrap());
        let mut traps = hooks.common_traps();

        let (completion_tx, completion_rx) = oneshot::channel();

        traps.extend(hooks.primary_traps(
            handle.clone(),
            joyflags.clone(),
            match_.clone(),
            CompletionToken {
                tx: std::sync::Arc::new(parking_lot::Mutex::new(Some(completion_tx))),
            },
        ));
        core.set_traps(traps);

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
            audio_binder.sample_rate(),
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
                core.set_keys(joyflags.load(std::sync::atomic::Ordering::Relaxed));
                emu_tps_counter.lock().mark();
            });
        }
        Ok(Session {
            vbuf,
            _audio_binding: audio_binding,
            thread,
            joyflags,
            mode: Mode::PvP(PvP {
                match_,
                cancellation_token,
            }),
            completion_rx,
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

        let game = game::find_by_rom_info(&core.as_mut().rom_code(), core.as_mut().rom_revision())
            .unwrap();
        let hooks = game.hooks();
        hooks.patch(core.as_mut());

        let (completion_tx, completion_rx) = oneshot::channel();

        let thread = mgba::thread::Thread::new(core);

        thread.start()?;
        thread
            .handle()
            .lock_audio()
            .sync_mut()
            .set_fps_target(EXPECTED_FPS);

        let audio_binding = audio_binder.bind(Some(Box::new(audio::MGBAStream::new(
            thread.handle(),
            audio_binder.sample_rate(),
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
                core.set_keys(joyflags.load(std::sync::atomic::Ordering::Relaxed));
                emu_tps_counter.lock().mark();
            });
        }
        Ok(Session {
            vbuf,
            _audio_binding: audio_binding,
            thread,
            joyflags,
            mode: Mode::SinglePlayer(SinglePlayer { completion_tx }),
            completion_rx,
        })
    }

    pub fn new_replayer(
        audio_binder: audio::LateBinder,
        rom: &[u8],
        emu_tps_counter: Arc<Mutex<stats::Counter>>,
        replay: &replay::Replay,
    ) -> Result<Self, anyhow::Error> {
        let mut core = mgba::core::Core::new_gba("tango")?;
        core.enable_video_buffer();

        core.as_mut()
            .load_rom(mgba::vfile::VFile::open_memory(&rom))?;

        let game = game::find_by_rom_info(&core.as_mut().rom_code(), core.as_mut().rom_revision())
            .unwrap();
        let hooks = game.hooks();
        hooks.patch(core.as_mut());

        let (completion_tx, completion_rx) = oneshot::channel();

        let input_pairs = replay.input_pairs.clone();
        let replayer_state = replayer::State::new(
            replay.local_player_index,
            input_pairs,
            0,
            Box::new(move || {
                let _ = completion_tx.send(());
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
            audio_binder.sample_rate(),
        ))))?;

        let local_state = replay.local_state.clone();
        thread.handle().run_on_core(move |mut core| {
            core.load_state(local_state.as_ref().unwrap())
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
                emu_tps_counter.lock().mark();
            });
        }

        Ok(Session {
            vbuf,
            _audio_binding: audio_binding,
            thread,
            joyflags: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            mode: Mode::Replayer,
            completion_rx,
        })
    }

    pub fn completed(&self) -> bool {
        match self.completion_rx.try_recv() {
            Err(oneshot::TryRecvError::Empty) => false,
            _ => true,
        }
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
