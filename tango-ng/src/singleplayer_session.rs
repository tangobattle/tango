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
    vbuf: Arc<Mutex<Vec<u8>>>,
    joyflags: Arc<AtomicU32>,
    close_requested: Arc<AtomicBool>,
    pause_on_next_frame: Arc<AtomicBool>,
    _thread: mgba::thread::Thread,
}

impl SinglePlayerSession {
    pub fn new(
        game: &'static (dyn crate::game::Game + Send + Sync),
        rom: Arc<Vec<u8>>,
        save_path: &std::path::Path,
    ) -> anyhow::Result<Self> {
        let mut core = mgba::core::Core::new_gba("tango-ng")?;
        core.enable_video_buffer();
        core.as_mut()
            .load_rom(mgba::vfile::VFile::from_vec(rom.as_ref().clone()))?;
        // Open RW so the game's own save writes persist back to disk —
        // mgba memory-maps the file and treats it as the cartridge SRAM.
        let save_file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(save_path)?;
        core.as_mut()
            .load_save(mgba::vfile::VFile::from_file(save_file))?;

        // hooks().patch installs the per-game memory patches that fix
        // determinism bugs (RNG seeding, RTC reads, etc.). Safe to apply
        // in single-player too — they don't depend on a partner.
        let hooks = game.hooks();
        hooks.patch(core.as_mut());

        let joyflags = Arc::new(AtomicU32::new(0));
        let pause_on_next_frame = Arc::new(AtomicBool::new(false));
        let thread = mgba::thread::Thread::new(core);
        let vbuf = Arc::new(Mutex::new(vec![
            0u8;
            (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT * 4) as usize
        ]));

        thread.set_frame_callback({
            let vbuf = vbuf.clone();
            let joyflags = joyflags.clone();
            let pause_on_next_frame = pause_on_next_frame.clone();
            move |mut core, video_buffer, mut thread_handle| {
                let mut vbuf = vbuf.lock();
                vbuf.copy_from_slice(video_buffer);
                fix_vbuf_alpha(&mut vbuf);
                core.set_keys(joyflags.load(Ordering::Relaxed));
                if pause_on_next_frame.swap(false, Ordering::SeqCst) {
                    thread_handle.pause();
                }
            }
        });

        thread.start()?;
        thread.handle().lock_audio().sync_mut().set_fps_target(EXPECTED_FPS);

        Ok(Self {
            vbuf,
            joyflags,
            close_requested: Arc::new(AtomicBool::new(false)),
            pause_on_next_frame,
            _thread: thread,
        })
    }

    pub fn snapshot_vbuf(&self) -> Vec<u8> {
        self.vbuf.lock().clone()
    }

    pub fn set_joyflag(&self, mgba_key_bit: u32, pressed: bool) {
        if pressed {
            self.joyflags.fetch_or(mgba_key_bit, Ordering::Relaxed);
        } else {
            self.joyflags.fetch_and(!mgba_key_bit, Ordering::Relaxed);
        }
    }

    pub fn request_close(&self) {
        self.close_requested.store(true, Ordering::SeqCst);
    }

    pub fn request_pause(&self) {
        self.pause_on_next_frame.store(true, Ordering::SeqCst);
    }
}

fn fix_vbuf_alpha(vbuf: &mut [u8]) {
    for px in vbuf.chunks_exact_mut(4) {
        px[3] = 0xFF;
    }
}

/// Default keyboard → mgba-key mapping. Mirrors the legacy app's
/// defaults (Z/X for A/B, A/S for L/R, Enter/Space for Start/Select,
/// arrow keys for the d-pad). Returns `None` for keys we don't bind.
pub fn key_to_mgba_bit(key: &iced::keyboard::Key) -> Option<u32> {
    use iced::keyboard::key::{Key, Named};
    use mgba::input::keys;
    match key {
        Key::Named(Named::ArrowLeft) => Some(keys::LEFT),
        Key::Named(Named::ArrowRight) => Some(keys::RIGHT),
        Key::Named(Named::ArrowUp) => Some(keys::UP),
        Key::Named(Named::ArrowDown) => Some(keys::DOWN),
        Key::Named(Named::Enter) => Some(keys::START),
        Key::Named(Named::Space) => Some(keys::SELECT),
        Key::Character(s) => match s.as_str() {
            "z" | "Z" => Some(keys::A),
            "x" | "X" => Some(keys::B),
            "a" | "A" => Some(keys::L),
            "s" | "S" => Some(keys::R),
            _ => None,
        },
        _ => None,
    }
}
