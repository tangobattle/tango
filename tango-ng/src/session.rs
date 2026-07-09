//! Emulator sessions — ports of `tango/src/session/{singleplayer,replay}.rs`
//! with the iced/tokio frame plumbing replaced by a polled dirty flag: the
//! mgba frame callback marks new frames and the Slint frame timer collects
//! them via `frame_dirty` + `read_frame`.
//!
//! Singleplayer installs no hooks (vanilla emulator ride); replay playback
//! installs the local game's stepper traps and feeds the recorded input
//! pairs from inside tango-pvp — the frontend never injects input there.
//! Replay seek/prefetch (SnapshotStore/SeekController) is not ported yet;
//! this is the minimal watch-start-to-end + pause + speed subset.

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

pub struct ReplaySession {
    stepper_state: tango_pvp::stepper::State,
    total_ticks: u32,
    completion: tango_pvp::hooks::CompletionToken,
    vbuf: Arc<Mutex<Vec<u8>>>,
    new_frame: Arc<AtomicBool>,
    // The shadow is shared with the stepper; keep our handle alive for
    // the session's lifetime (and for the eventual seek port).
    _shadow: tango_pvp::stepper::SharedShadow,
    // Binding drops before the thread (declaration order), so the audio
    // mux reverts to silence before the emu thread is torn down.
    _audio_binding: Option<crate::audio::Binding>,
    _thread: mgba::thread::Thread,
}

impl ReplaySession {
    pub fn new(
        replay: tango_pvp::replay::Replay,
        local_game: tango_gamesupport::GameRef,
        local_rom: &[u8],
        remote_game: tango_gamesupport::GameRef,
        remote_rom: &[u8],
        audio_binder: &crate::audio::LateBinder,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(!replay.rounds.is_empty(), "replay has no rounds");

        let mut core = new_gba_core(local_rom)?;
        // In-memory SRAM clone — playback must never touch a real save.
        core.as_mut()
            .load_save(mgba::vfile::VFile::from_vec(replay.local_sram.clone()))?;
        // Pin cart RTC to the recorded match clock (before the thread
        // starts/resets); RTC-reading games desync without it.
        core.set_rtc_fixed(replay.rtc_time());

        let completion = tango_pvp::hooks::CompletionToken::new();
        let replay_is_complete = replay.is_complete;
        let total_ticks: u32 = replay.rounds.iter().map(|r| r.len() as u32).sum();

        let (stepper_state, shadow) = tango_pvp::stepper::State::new_for_replay(
            &replay,
            remote_rom,
            remote_game.hooks,
            Box::new({
                let completion = completion.clone();
                move || completion.complete()
            }),
        )?;
        local_game.hooks.install_on_stepper(&mut core, stepper_state.clone());

        let vbuf = Arc::new(Mutex::new(vec![
            0u8;
            SCREEN_WIDTH as usize * SCREEN_HEIGHT as usize * 2
        ]));
        let new_frame = Arc::new(AtomicBool::new(false));
        let thread = mgba::thread::Thread::new(core);

        thread.set_frame_callback({
            let vbuf = vbuf.clone();
            let new_frame = new_frame.clone();
            let stepper_state = stepper_state.clone();
            let completion = completion.clone();
            move |_core, video_buffer, mut thread_handle| {
                let (total_left, is_round_ended) = {
                    let mut inner = stepper_state.lock_inner();
                    if let Some(err) = inner.take_error() {
                        log::error!("replay stepper error: {err:?}");
                    }
                    (inner.total_input_pairs_left(), inner.is_round_ended())
                };

                vbuf.lock().unwrap().copy_from_slice(video_buffer);
                new_frame.store(true, Ordering::Release);

                // Clean replays wait for the round-end animation; incomplete
                // ones fall through on input exhaustion.
                if total_left == 0 && (is_round_ended || !replay_is_complete) {
                    completion.complete();
                }
                if completion.is_complete() {
                    thread_handle.pause();
                }
            }
        });

        thread.start()?;
        thread.handle().lock_audio().sync_mut().set_fps_target(EXPECTED_FPS);

        let audio_binding = audio_binder.bind_mgba(thread.handle(), "replay");

        Ok(Self {
            stepper_state,
            total_ticks,
            completion,
            vbuf,
            new_frame,
            _shadow: shadow,
            _audio_binding: audio_binding,
            _thread: thread,
        })
    }

    pub fn is_paused(&self) -> bool {
        self._thread.handle().is_paused()
    }

    pub fn set_paused(&self, paused: bool) {
        let handle = self._thread.handle();
        if paused {
            handle.pause();
        } else {
            handle.unpause();
        }
    }

    pub fn set_speed(&self, factor: f32) {
        let fps = (EXPECTED_FPS * factor).clamp(1.0, EXPECTED_FPS * 4.0);
        self._thread.handle().lock_audio().sync_mut().set_fps_target(fps);
    }

    /// Recorded-frame index of the playhead (freezes during the
    /// input-less inter-round animation).
    pub fn current_tick(&self) -> u32 {
        self.stepper_state.lock_inner().inputs_consumed()
    }

    pub fn total_ticks(&self) -> u32 {
        self.total_ticks
    }

    pub fn is_complete(&self) -> bool {
        self.completion.is_complete()
    }

    pub fn frame_dirty(&self) -> bool {
        self.new_frame.swap(false, Ordering::AcqRel)
    }

    pub fn read_frame(&self, dst_rgba: &mut [u8]) {
        let vbuf = self.vbuf.lock().unwrap();
        tango_dataview::rom::bgr555_to_rgba8(&vbuf, dst_rgba);
    }
}
