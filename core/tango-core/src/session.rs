use crate::{audio, battle, game, hooks, ipc, stats};
use parking_lot::Mutex;
use rand::SeedableRng;
use std::sync::Arc;

pub struct Session {
    vbuf: std::sync::Arc<Mutex<Vec<u8>>>,
    _audio_binding: audio::Binding<i16>,
    thread: mgba::thread::Thread,
    joyflags: std::sync::Arc<std::sync::atomic::AtomicU32>,
    match_: Option<std::sync::Arc<tokio::sync::Mutex<Option<std::sync::Arc<battle::Match>>>>>,
}

impl Session {
    pub fn new(
        handle: tokio::runtime::Handle,
        ipc_sender: Arc<Mutex<ipc::Sender>>,
        audio_cb: audio::LateBinder<i16>,
        audio_spec: &sdl2::audio::AudioSpec,
        rom_path: std::path::PathBuf,
        save_path: std::path::PathBuf,
        emu_tps_counter: Arc<Mutex<stats::Counter>>,
        match_init: Option<battle::MatchInit>,
    ) -> Result<Self, anyhow::Error> {
        let mut core = mgba::core::Core::new_gba("tango")?;
        core.enable_video_buffer();

        let rom = std::fs::read(rom_path)?;
        let rom_vf = mgba::vfile::VFile::open_memory(&rom);
        core.as_mut().load_rom(rom_vf)?;

        let save_vf = if match_init.is_none() {
            mgba::vfile::VFile::open(
                &save_path,
                mgba::vfile::flags::O_CREAT | mgba::vfile::flags::O_RDWR,
            )?
        } else {
            log::info!("in pvp mode, save file will not be written back to disk");
            mgba::vfile::VFile::open_memory(&std::fs::read(save_path)?)
        };

        core.as_mut().load_save(save_vf)?;

        let joyflags = Arc::new(std::sync::atomic::AtomicU32::new(0));

        let hooks = hooks::get(core.as_mut()).unwrap();
        hooks.patch(core.as_mut());

        let match_ = std::sync::Arc::new(tokio::sync::Mutex::new(None));
        if let Some(match_init) = match_init.as_ref() {
            let _ = std::fs::create_dir_all(match_init.settings.replays_path.parent().unwrap());
            let mut traps = hooks.common_traps();
            traps.extend(hooks.primary_traps(handle.clone(), joyflags.clone(), match_.clone()));
            core.set_traps(traps);
        }

        let thread = mgba::thread::Thread::new(core);

        let match_ = if let Some(match_init) = match_init {
            let (dc_rx, dc_tx) = match_init.dc.split();

            {
                let match_ = match_.clone();
                handle.block_on(async {
                    let is_offerer = match_init.peer_conn.local_description().unwrap().sdp_type
                        == datachannel_wrapper::SdpType::Offer;
                    let rng_seed = match_init
                        .settings
                        .rng_seed
                        .clone()
                        .try_into()
                        .expect("rng seed");
                    *match_.lock().await = Some(
                        battle::Match::new(
                            rom,
                            hooks,
                            match_init.peer_conn,
                            dc_tx,
                            rand_pcg::Mcg128Xsl64::from_seed(rng_seed),
                            is_offerer,
                            thread.handle(),
                            ipc_sender.clone(),
                            match_init.settings,
                        )
                        .expect("new match"),
                    );
                });
            }

            {
                let match_ = match_.clone();
                handle.spawn(async move {
                    {
                        let match_ = match_.lock().await.clone().unwrap();
                        tokio::select! {
                            Err(e) = match_.run(dc_rx) => {
                                log::info!("match thread ending: {:?}", e);
                            }
                            _ = match_.cancelled() => {
                            }
                        }
                    }
                });
            }

            Some(match_)
        } else {
            None
        };

        thread.start()?;
        thread
            .handle()
            .lock_audio()
            .sync_mut()
            .set_fps_target(game::EXPECTED_FPS);

        let audio_binding = audio_cb.bind(Some(Box::new(audio::MGBAStream::new(
            thread.handle(),
            audio_spec.freq,
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
            match_,
        })
    }

    pub fn match_(
        &self,
    ) -> &Option<std::sync::Arc<tokio::sync::Mutex<Option<std::sync::Arc<battle::Match>>>>> {
        &self.match_
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
