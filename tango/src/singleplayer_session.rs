//! Standalone (no-netplay) emulator session. Boots a ROM with the
//! user-selected save file and accepts joyflag input from the UI tick
//! loop. The video frame plumbing mirrors `replay_session` — same
//! Arc<Mutex<Vec<u8>>> vbuf, same fix_vbuf_alpha pass.
//!
//! No hooks::Hooks traps are installed: this is a vanilla emulator
//! ride for one player. (The PVP / replay traps require a partner /
//! recorded packets, neither of which apply here.)

use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

const EXPECTED_FPS: f32 = 60.0;

pub struct SinglePlayerSession {
    game: &'static crate::game::Game,
    vbuf: Arc<Mutex<Vec<u8>>>,
    joyflags: Arc<AtomicU32>,
    close_requested: Arc<AtomicBool>,
    /// Bumped once per emulator frame in the frame callback. The UI
    /// tick reads this to decide whether to rebuild the iced
    /// `Handle` (which would otherwise re-upload the same texture
    /// to the GPU on every vsync of a high-refresh display).
    frame_id: Arc<std::sync::atomic::AtomicU64>,
    _audio_binding: Option<crate::audio::Binding>,
    _thread: mgba::thread::Thread,
}

impl SinglePlayerSession {
    pub fn new(
        game: &'static crate::game::Game,
        rom: Arc<Vec<u8>>,
        save_path: &std::path::Path,
        audio_binder: &crate::audio::LateBinder,
    ) -> anyhow::Result<Self> {
        let mut core = mgba::core::Core::new_gba("tango")?;
        core.enable_video_buffer();
        core.as_mut()
            .load_rom(mgba::vfile::VFile::from_vec(rom.as_ref().clone()))?;
        // Open RW so the game's own save writes persist back to disk —
        // mgba memory-maps the file and treats it as the cartridge SRAM.
        let save_file = std::fs::OpenOptions::new().read(true).write(true).open(save_path)?;
        core.as_mut().load_save(mgba::vfile::VFile::from_file(save_file))?;

        // hooks().patch installs the per-game memory patches that fix
        // determinism bugs (RNG seeding, RTC reads, etc.). Safe to apply
        // in single-player too — they don't depend on a partner.
        let hooks = game.hooks;
        hooks.patch(core.as_mut());

        let joyflags = Arc::new(AtomicU32::new(0));
        let frame_id = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let thread = mgba::thread::Thread::new(core);
        let vbuf = Arc::new(Mutex::new(vec![
            0u8;
            (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT * 4)
                as usize
        ]));

        thread.set_frame_callback({
            let vbuf = vbuf.clone();
            let joyflags = joyflags.clone();
            let frame_id = frame_id.clone();
            move |mut core, video_buffer, _thread_handle| {
                let mut vbuf = vbuf.lock();
                vbuf.copy_from_slice(video_buffer);
                fix_vbuf_alpha(&mut vbuf);
                core.set_keys(joyflags.load(Ordering::Relaxed));
                frame_id.fetch_add(1, Ordering::Release);
            }
        });

        thread.start()?;
        thread.handle().lock_audio().sync_mut().set_fps_target(EXPECTED_FPS);

        let audio_binding = match audio_binder.bind(Some(Box::new(crate::audio::MGBAStream::new(
            thread.handle(),
            audio_binder.sample_rate(),
        )))) {
            Ok(b) => Some(b),
            Err(e) => {
                log::warn!("singleplayer: audio bind failed: {e:?}");
                None
            }
        };

        Ok(Self {
            game,
            vbuf,
            joyflags,
            close_requested: Arc::new(AtomicBool::new(false)),
            frame_id,
            _audio_binding: audio_binding,
            _thread: thread,
        })
    }

    pub fn game(&self) -> &'static crate::game::Game {
        self.game
    }

    /// Monotonically-increasing counter of frames the emulator
    /// has rendered. Read by the UI tick to skip redundant
    /// texture uploads when the host vsync is faster than mgba.
    pub fn frame_id(&self) -> u64 {
        self.frame_id.load(Ordering::Acquire)
    }

    pub fn snapshot_vbuf(&self) -> Vec<u8> {
        self.vbuf.lock().clone()
    }

    /// Overwrite the entire mgba joyflag bitmap — the configurable
    /// input mapping resolves multiple held bindings into one
    /// flag word and pushes the result here every event.
    pub fn set_joyflags(&self, mgba_keys: u32) {
        self.joyflags.store(mgba_keys, Ordering::Relaxed);
    }

    pub fn request_close(&self) {
        self.close_requested.store(true, Ordering::SeqCst);
    }

    /// Drive the emulator at `factor * EXPECTED_FPS` fps. 1.0 = realtime,
    /// >1.0 = fast-forward. Audio paces frames, so values above ~4x
    /// start dropping samples; clamp accordingly to keep audio coherent.
    pub fn set_speed(&self, factor: f32) {
        let fps = (EXPECTED_FPS * factor).clamp(1.0, EXPECTED_FPS * 4.0);
        self._thread.handle().lock_audio().sync_mut().set_fps_target(fps);
    }
}

fn fix_vbuf_alpha(vbuf: &mut [u8]) {
    for px in vbuf.chunks_exact_mut(4) {
        px[3] = 0xFF;
    }
}
