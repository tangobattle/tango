//! Standalone (no-netplay) emulator session. Boots a ROM with the
//! user-selected save file and accepts joyflag input from the UI tick
//! loop. The video frame plumbing mirrors the other sessions — the
//! drive loop writes mgba's raw BGR555 into the session's own
//! [`Framebuffer`](crate::Framebuffer) (the framebuffer shader expands
//! it to RGB on the GPU).
//!
//! The core runs on a drive thread we own (mgba is built without its
//! thread runner), paced by the wall clock the same way the PvP drive
//! loop paces itself: `next_tick` accumulates absolute 1/fps deadlines
//! (drift-free on average), and a loop that falls far behind
//! resynchronizes its cadence instead of sprinting. Audio follows as a
//! pure consumer through the shared
//! [`CoreStream`](crate::core_stream::CoreStream) rate
//! control, so a stalled or torn-down audio device costs sound, never
//! the session.
//!
//! No hooks::Hooks traps are installed: this is a vanilla emulator
//! ride for one player. (The PVP / replay traps require a partner /
//! recorded packets, neither of which apply here.)

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

const EXPECTED_FPS: f32 = 60.0;

/// State shared between the drive thread and the audio stream.
struct Shared {
    core: Mutex<mgba::core::OwnedCore>,
    /// Drive pacing target as f32 bits. 60.0 = realtime; fast-forward
    /// raises it and the audio stream's faux clock compresses to match.
    fps_bits: AtomicU32,
    stop: AtomicBool,
}

/// Cross-thread audio pull over the session core's mutex — the drive
/// thread holds it only while stepping a frame, so the callback's
/// readout interleaves between ticks.
struct SharedCorePull(Arc<Shared>);

impl crate::core_stream::CorePull for SharedCorePull {
    fn with_core(&self, f: &mut dyn FnMut(&mut mgba::core::Core)) {
        f(&mut self.0.core.lock().unwrap());
    }
}

pub struct SinglePlayerSession {
    game: &'static tango_gamesupport::Game,
    joyflags: Arc<AtomicU32>,
    shared: Arc<Shared>,
    screen: Arc<crate::Framebuffer>,
    wake: Arc<tokio::sync::Notify>,
    drive: Option<std::thread::JoinHandle<()>>,
}

impl SinglePlayerSession {
    /// Boot the session. Also returns its audio stream — the session's
    /// samples resampled to `sample_rate` — for the host to route to its
    /// output; dropping it just costs sound (the wall-clock pacer doesn't
    /// depend on the audio device at all).
    pub fn new(
        game: &'static tango_gamesupport::Game,
        rom: Arc<Vec<u8>>,
        save_path: &std::path::Path,
        sample_rate: u32,
    ) -> Result<(Self, crate::core_stream::CoreStream), crate::Error> {
        let mut core = crate::new_gba_core(rom.as_ref())?;
        // Open RW so the game's own save writes persist back to disk —
        // mgba memory-maps the file and treats it as the cartridge SRAM.
        let save_file = std::fs::OpenOptions::new().read(true).write(true).open(save_path)?;
        core.load_save(mgba::vfile::VFile::from_file(save_file))?;
        core.reset();
        // Queue headroom for the stream's rate control — the discard cap
        // sits at 3x its 50 ms target and fast-forward piles up several
        // callbacks' worth between fills; mGBA's default buffer doesn't
        // hold that at BN4+'s 65536 Hz. Same sizing as the pair engine.
        core.set_audio_buffer_size(16384);

        let joyflags = Arc::new(AtomicU32::new(0));
        let shared = Arc::new(Shared {
            core: Mutex::new(core),
            fps_bits: AtomicU32::new(EXPECTED_FPS.to_bits()),
            stop: AtomicBool::new(false),
        });

        let screen = crate::Framebuffer::new();
        let wake = Arc::new(tokio::sync::Notify::new());
        let drive = std::thread::Builder::new().name("singleplayer".to_owned()).spawn({
            let shared = shared.clone();
            let joyflags = joyflags.clone();
            let screen = screen.clone();
            let wake = wake.clone();
            move || drive_loop(shared, joyflags, screen, wake)
        })?;

        let audio = crate::core_stream::CoreStream::new(
            SharedCorePull(shared.clone()),
            {
                let shared = shared.clone();
                move || f32::from_bits(shared.fps_bits.load(Ordering::Relaxed))
            },
            sample_rate,
        );

        Ok((
            Self {
                game,
                joyflags,
                shared,
                screen,
                wake,
                drive: Some(drive),
            },
            audio,
        ))
    }
}

impl crate::ActiveSession for SinglePlayerSession {
    fn local_game(&self) -> &'static tango_gamesupport::Game {
        self.game
    }

    fn screen(&self) -> Arc<crate::Framebuffer> {
        self.screen.clone()
    }

    fn wake(&self) -> Arc<tokio::sync::Notify> {
        self.wake.clone()
    }

    fn set_joyflags(&self, joyflags: u32) {
        self.joyflags.store(joyflags, Ordering::Relaxed);
    }

    /// Above ~4x, one callback interval's production overshoots the
    /// stream's discard cap and fast-forward audio turns into constant
    /// skips; clamp to keep it coherent.
    fn set_speed(&self, factor: f32) {
        let fps = (EXPECTED_FPS * factor).clamp(1.0, EXPECTED_FPS * 4.0);
        self.shared.fps_bits.store(fps.to_bits(), Ordering::Relaxed);
    }
}

impl Drop for SinglePlayerSession {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(drive) = self.drive.take() {
            let _ = drive.join();
        }
    }
}

fn drive_loop(
    shared: Arc<Shared>,
    joyflags: Arc<AtomicU32>,
    screen: Arc<crate::Framebuffer>,
    wake: Arc<tokio::sync::Notify>,
) {
    let mut next_tick = std::time::Instant::now();
    while !shared.stop.load(Ordering::Relaxed) {
        {
            // Scoped: the audio callback pulls samples under this same
            // mutex, so it must be free while we sleep off the tick.
            let mut core = shared.core.lock().unwrap();
            core.set_keys(joyflags.load(Ordering::Relaxed));
            core.run_frame();
            if let Some(frame) = core.video_buffer() {
                // mgba's native BGR555 goes up as-is; the framebuffer
                // shader expands it to RGB on the GPU at draw time.
                screen.write(frame);
            }
        }
        // Wake the host's frame subscription so the UI rebuilds the
        // texture for this frame. Notify coalesces — a slow UI doesn't
        // queue up wakes.
        wake.notify_one();

        let mut fps = f32::from_bits(shared.fps_bits.load(Ordering::Relaxed));
        if fps <= 0.0 {
            fps = EXPECTED_FPS;
        }
        next_tick += std::time::Duration::from_secs_f64(1.0 / fps as f64);
        let now = std::time::Instant::now();
        if next_tick > now {
            std::thread::sleep(next_tick - now);
        } else if now - next_tick > std::time::Duration::from_millis(250) {
            // Fell way behind (debugger, laptop lid, ...): don't sprint
            // to catch up, just resynchronize the cadence.
            next_tick = now;
        }
    }
}
