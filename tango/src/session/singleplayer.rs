//! Standalone (no-netplay) emulator session. Boots a ROM with the
//! user-selected save file and accepts joyflag input from the UI tick
//! loop. The video frame plumbing mirrors `replay_session` — the same
//! Arc<Mutex<Vec<u8>>> vbuf, fed mgba's raw BGR555 (the framebuffer shader
//! expands it on the GPU).
//!
//! No hooks::Hooks traps are installed: this is a vanilla emulator
//! ride for one player. (The PVP / replay traps require a partner /
//! recorded packets, neither of which apply here.)

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::sync::Mutex;

const EXPECTED_FPS: f32 = 60.0;

pub struct SinglePlayerSession {
    game: &'static crate::library::game::Game,
    joyflags: Arc<AtomicU32>,
    _audio_binding: Option<crate::platform::audio::Binding>,
    _thread: mgba::thread::Thread,
}

impl SinglePlayerSession {
    pub fn new(
        game: &'static crate::library::game::Game,
        rom: Arc<Vec<u8>>,
        save_path: &std::path::Path,
        audio_binder: &crate::platform::audio::LateBinder,
        frame_notify: Arc<tokio::sync::Notify>,
        vbuf: Arc<Mutex<Vec<u8>>>,
    ) -> anyhow::Result<Self> {
        let mut core = crate::session::new_gba_core(rom.as_ref())?;
        // Open RW so the game's own save writes persist back to disk —
        // mgba memory-maps the file and treats it as the cartridge SRAM.
        let save_file = std::fs::OpenOptions::new().read(true).write(true).open(save_path)?;
        core.as_mut().load_save(mgba::vfile::VFile::from_file(save_file))?;

        let joyflags = Arc::new(AtomicU32::new(0));
        let thread = mgba::thread::Thread::new(core);
        // Wipe the shared framebuffer so the previous session's
        // last frame doesn't flash through before mgba writes its
        // first one. The post-constructor `current_frame` clear
        // covers iced's side; this covers the source.
        vbuf.lock().unwrap().fill(0);

        thread.set_frame_callback({
            let vbuf = vbuf.clone();
            let joyflags = joyflags.clone();
            let frame_notify = frame_notify.clone();
            move |mut core, video_buffer, _thread_handle| {
                // Copy mgba's native BGR555 straight through; the framebuffer
                // shader expands it to RGB on the GPU at draw time.
                vbuf.lock().unwrap().copy_from_slice(video_buffer);
                core.set_keys(joyflags.load(Ordering::Relaxed));
                // Wake the session subscription so iced rebuilds
                // the texture handle for this frame. Notify
                // coalesces — a slow UI doesn't queue up wakes.
                frame_notify.notify_one();
            }
        });

        thread.start()?;
        thread.handle().lock_audio().sync_mut().set_fps_target(EXPECTED_FPS);

        let audio_binding = audio_binder.bind_mgba(thread.handle(), "singleplayer");

        Ok(Self {
            game,
            joyflags,
            _audio_binding: audio_binding,
            _thread: thread,
        })
    }

    pub fn game(&self) -> &'static crate::library::game::Game {
        self.game
    }

    /// Overwrite the entire mgba joyflag bitmap — the configurable
    /// input mapping resolves multiple held bindings into one
    /// flag word and pushes the result here every event.
    pub fn set_joyflags(&self, mgba_keys: u32) {
        self.joyflags.store(mgba_keys, Ordering::Relaxed);
    }

    /// Drive the emulator at `factor * EXPECTED_FPS` fps. 1.0 = realtime,
    /// anything higher = fast-forward. Audio paces frames, so values
    /// above ~4x start dropping samples; clamp accordingly to keep audio
    /// coherent.
    pub fn set_speed(&self, factor: f32) {
        let fps = (EXPECTED_FPS * factor).clamp(1.0, EXPECTED_FPS * 4.0);
        self._thread.handle().lock_audio().sync_mut().set_fps_target(fps);
    }
}
