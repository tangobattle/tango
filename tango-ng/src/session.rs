//! Standalone (no-netplay) emulator session — a port of
//! `tango/src/session/singleplayer.rs` with the iced/tokio frame plumbing
//! replaced by a polled dirty flag: the mgba frame callback marks new
//! frames and the Slint frame timer collects them via
//! [`SinglePlayerSession::frame_dirty`] + [`SinglePlayerSession::read_frame`].
//!
//! No hooks::Hooks traps are installed: this is a vanilla emulator ride
//! for one player.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

pub const SCREEN_WIDTH: u32 = mgba::gba::SCREEN_WIDTH;
pub const SCREEN_HEIGHT: u32 = mgba::gba::SCREEN_HEIGHT;

const EXPECTED_FPS: f32 = 60.0;

/// Create the mgba core every session boots from: a GBA core with audio-sync
/// on, its video buffer enabled, and `rom` loaded. (Same shape as the tango
/// crate's `new_gba_core`; PvP/replay sessions will layer their traps on top
/// when they're ported.)
fn new_gba_core(rom: &[u8]) -> anyhow::Result<mgba::core::Core> {
    let mut core = mgba::core::Core::new_gba(
        "tango",
        &mgba::core::Options {
            audio_sync: true,
            ..Default::default()
        },
    )?;
    core.enable_video_buffer();
    core.as_mut().load_rom(mgba::vfile::VFile::from_vec(rom.to_vec()))?;
    Ok(core)
}

pub struct SinglePlayerSession {
    joyflags: Arc<AtomicU32>,
    vbuf: Arc<Mutex<Vec<u8>>>,
    new_frame: Arc<AtomicBool>,
    _audio_binding: Option<crate::audio::Binding>,
    _thread: mgba::thread::Thread,
}

impl SinglePlayerSession {
    pub fn new(
        rom: &[u8],
        save_path: &std::path::Path,
        audio_binder: &crate::audio::LateBinder,
    ) -> anyhow::Result<Self> {
        let mut core = new_gba_core(rom)?;
        // Open RW so the game's own save writes persist back to disk —
        // mgba memory-maps the file and treats it as the cartridge SRAM.
        let save_file = std::fs::OpenOptions::new().read(true).write(true).open(save_path)?;
        core.as_mut().load_save(mgba::vfile::VFile::from_file(save_file))?;

        let joyflags = Arc::new(AtomicU32::new(0));
        let vbuf = Arc::new(Mutex::new(vec![
            0u8;
            SCREEN_WIDTH as usize * SCREEN_HEIGHT as usize * 2
        ]));
        let new_frame = Arc::new(AtomicBool::new(false));
        let thread = mgba::thread::Thread::new(core);

        thread.set_frame_callback({
            let vbuf = vbuf.clone();
            let joyflags = joyflags.clone();
            let new_frame = new_frame.clone();
            move |mut core, video_buffer, _thread_handle| {
                // Copy mgba's native BGR555 straight through; the UI side
                // expands it to RGBA8 when it collects the frame.
                vbuf.lock().unwrap().copy_from_slice(video_buffer);
                core.set_keys(joyflags.load(Ordering::Relaxed));
                new_frame.store(true, Ordering::Release);
            }
        });

        thread.start()?;
        thread.handle().lock_audio().sync_mut().set_fps_target(EXPECTED_FPS);

        let audio_binding = audio_binder.bind_mgba(thread.handle(), "singleplayer");

        Ok(Self {
            joyflags,
            vbuf,
            new_frame,
            _audio_binding: audio_binding,
            _thread: thread,
        })
    }

    /// Overwrite the entire mgba joyflag bitmap — the input mapping
    /// resolves multiple held bindings into one flag word and pushes
    /// the result here every event.
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

    /// True (and self-clearing) when a frame arrived since the last call.
    pub fn frame_dirty(&self) -> bool {
        self.new_frame.swap(false, Ordering::AcqRel)
    }

    /// Convert the most recent frame to RGBA8 into `dst_rgba`
    /// (`SCREEN_WIDTH * SCREEN_HEIGHT * 4` bytes).
    pub fn read_frame(&self, dst_rgba: &mut [u8]) {
        let vbuf = self.vbuf.lock().unwrap();
        tango_dataview::rom::bgr555_to_rgba8(&vbuf, dst_rgba);
    }
}
