//! Standalone (no-netplay) emulator session. Boots a ROM with the
//! user-selected save file and accepts joyflag input from the UI tick
//! loop. The video frame plumbing mirrors the other sessions — the
//! session's own [`FrameSink`](crate::session::FrameSink) vbuf, fed
//! mgba's raw BGR555 (the framebuffer shader expands it on the GPU).
//!
//! The core runs on a drive thread we own (mgba is built without its
//! thread runner), paced by the audio clock exactly as mgba's own sync
//! paced it: the audio fill computes a high-water mark from its callback
//! size, and the drive thread runs frames only while the core's sample
//! queue sits below it, sleeping until the fill consumes. A stalled
//! audio device therefore freezes the session — the accepted tradeoff
//! of audio-clock sync.
//!
//! No hooks::Hooks traps are installed: this is a vanilla emulator
//! ride for one player. (The PVP / replay traps require a partner /
//! recorded packets, neither of which apply here.)

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};

const EXPECTED_FPS: f32 = 60.0;

/// State shared between the drive thread and the audio stream.
struct Shared {
    core: Mutex<mgba::core::OwnedCore>,
    /// Signaled by the audio fill after it consumes samples (and by
    /// drop, to release a final wait).
    consumed: Condvar,
    /// How many source sample frames the core may keep queued before
    /// the drive thread waits. Recomputed by every audio fill from its
    /// callback size and the current rates (mGBA's SDL frontend
    /// formula, as the old thread-sync path used); the initial value
    /// just bounds the pre-first-fill run.
    high_water: AtomicU32,
    /// Drive pacing target as f32 bits. 60.0 = realtime; fast-forward
    /// raises it and the audio fill stretches its faux clock to match.
    fps_bits: AtomicU32,
    stop: AtomicBool,
}

pub struct SinglePlayerSession {
    game: &'static crate::library::game::Game,
    joyflags: Arc<AtomicU32>,
    shared: Arc<Shared>,
    frame_sink: crate::session::FrameSink,
    _audio_binding: Option<crate::platform::audio::Binding>,
    drive: Option<std::thread::JoinHandle<()>>,
}

impl SinglePlayerSession {
    pub fn new(
        game: &'static crate::library::game::Game,
        rom: Arc<Vec<u8>>,
        save_path: &std::path::Path,
        audio_binder: &crate::platform::audio::LateBinder,
    ) -> anyhow::Result<Self> {
        let mut core = crate::session::new_gba_core(rom.as_ref())?;
        // Open RW so the game's own save writes persist back to disk —
        // mgba memory-maps the file and treats it as the cartridge SRAM.
        let save_file = std::fs::OpenOptions::new().read(true).write(true).open(save_path)?;
        core.load_save(mgba::vfile::VFile::from_file(save_file))?;
        core.reset();

        let joyflags = Arc::new(AtomicU32::new(0));
        let shared = Arc::new(Shared {
            core: Mutex::new(core),
            consumed: Condvar::new(),
            high_water: AtomicU32::new(crate::platform::audio::SAMPLES as u32 * 2),
            fps_bits: AtomicU32::new(EXPECTED_FPS.to_bits()),
            stop: AtomicBool::new(false),
        });

        let frame_sink = crate::session::FrameSink::new();
        let drive = std::thread::Builder::new().name("singleplayer".to_owned()).spawn({
            let shared = shared.clone();
            let joyflags = joyflags.clone();
            let vbuf = frame_sink.vbuf.clone();
            let frame_notify = frame_sink.notify.clone();
            move || drive_loop(shared, joyflags, vbuf, frame_notify)
        })?;

        // A failed bind is logged and downgraded to silence rather than
        // aborting the session (the high-water gate then paces on the
        // initial mark alone — wrong speed beats no session).
        let audio_binding = match audio_binder.bind(Some(Box::new(SinglePlayerStream {
            shared: shared.clone(),
            out_rate: audio_binder.sample_rate(),
            resampler: mgba::audio::AudioResampler::new(),
            dest_buffer: mgba::audio::OwnedAudioBuffer::new(
                crate::platform::audio::SAMPLES * 2,
                crate::platform::audio::NUM_CHANNELS as u32,
            ),
            dest_capacity: crate::platform::audio::SAMPLES * 2,
        }))) {
            Ok(b) => Some(b),
            Err(e) => {
                log::warn!("singleplayer: audio bind failed: {e:?}");
                None
            }
        };

        Ok(Self {
            game,
            joyflags,
            shared,
            frame_sink,
            _audio_binding: audio_binding,
            drive: Some(drive),
        })
    }
}

impl crate::session::ActiveSession for SinglePlayerSession {
    fn local_game(&self) -> &'static crate::library::game::Game {
        self.game
    }

    fn frame_sink(&self) -> &crate::session::FrameSink {
        &self.frame_sink
    }

    fn view<'a>(&'a self, ctx: crate::session::view::Ctx<'a>) -> iced::Element<'a, crate::session::Message> {
        crate::session::view::singleplayer::view(self, ctx)
    }

    fn set_joyflags(&self, joyflags: u32) {
        self.joyflags.store(joyflags, Ordering::Relaxed);
    }

    /// Audio paces frames, so factors above ~4x start dropping
    /// samples; clamp accordingly to keep audio coherent.
    fn set_speed(&self, factor: f32) {
        let fps = (EXPECTED_FPS * factor).clamp(1.0, EXPECTED_FPS * 4.0);
        self.shared.fps_bits.store(fps.to_bits(), Ordering::Relaxed);
    }
}

impl Drop for SinglePlayerSession {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        self.shared.consumed.notify_all();
        if let Some(drive) = self.drive.take() {
            let _ = drive.join();
        }
    }
}

fn drive_loop(
    shared: Arc<Shared>,
    joyflags: Arc<AtomicU32>,
    vbuf: Arc<Mutex<Vec<u8>>>,
    frame_notify: Arc<tokio::sync::Notify>,
) {
    let mut core = shared.core.lock().unwrap();
    loop {
        // Audio-clock pacing: wait while the queue is above high water.
        // The timeout is not a pacing device — it only covers the
        // notify-while-not-yet-waiting race on stop and lets a rebound
        // audio device pick the session back up.
        while !shared.stop.load(Ordering::Relaxed)
            && core.audio_buffer().available() as u32 > shared.high_water.load(Ordering::Relaxed)
        {
            (core, _) = shared
                .consumed
                .wait_timeout(core, std::time::Duration::from_millis(250))
                .unwrap();
        }
        if shared.stop.load(Ordering::Relaxed) {
            return;
        }

        core.set_keys(joyflags.load(Ordering::Relaxed));
        core.run_frame();

        if let Some(frame) = core.video_buffer() {
            // Copy mgba's native BGR555 straight through; the framebuffer
            // shader expands it to RGB on the GPU at draw time.
            vbuf.lock().unwrap().copy_from_slice(frame);
        }
        // Wake the session subscription so iced rebuilds
        // the texture handle for this frame. Notify
        // coalesces — a slow UI doesn't queue up wakes.
        frame_notify.notify_one();
    }
}

/// Pulls audio out of the session core, resampling from mGBA's internal
/// rate to the host audio rate, and publishes the high-water mark the
/// drive thread paces on. The high-water formula follows mGBA's SDL
/// frontend so high-SOUNDBIAS games (Battle Network 4+) don't starve.
struct SinglePlayerStream {
    shared: Arc<Shared>,
    out_rate: u32,
    resampler: mgba::audio::AudioResampler,
    dest_buffer: mgba::audio::OwnedAudioBuffer,
    /// Tracked separately because `mAudioBuffer` doesn't expose
    /// capacity through the Rust binding; grown lazily in `fill`.
    dest_capacity: usize,
}

impl crate::platform::audio::Stream for SinglePlayerStream {
    fn fill(&mut self, buf: &mut [[i16; crate::platform::audio::NUM_CHANNELS]]) -> usize {
        let frame_count = buf.len();
        let linear_buf: &mut [i16] = bytemuck::cast_slice_mut(buf);

        let needed = frame_count.saturating_mul(2);
        if needed > self.dest_capacity {
            let new_capacity = needed.next_power_of_two().max(crate::platform::audio::SAMPLES * 2);
            self.dest_buffer =
                mgba::audio::OwnedAudioBuffer::new(new_capacity, crate::platform::audio::NUM_CHANNELS as u32);
            self.dest_capacity = new_capacity;
        }

        let mut fps_target = f32::from_bits(self.shared.fps_bits.load(Ordering::Relaxed));
        if fps_target <= 0.0 {
            fps_target = EXPECTED_FPS;
        }

        let mut core = self.shared.core.lock().unwrap();
        // The core's production rate follows the game's SOUNDBIAS
        // resolution and CHANGES at runtime (BN4+ flip from 32768 to
        // 65536 Hz after boot), so it's re-read every fill.
        let core_rate = core.audio_sample_rate() as f64;
        // The faux clock: production scales with the drive's pace, so a
        // fast-forwarded core stretches into the same output rate
        // instead of flooding it.
        let faux_clock = core.calculate_framerate_ratio(fps_target as f64);
        let dest_rate = self.out_rate as f64 * faux_clock;

        let high_water = (frame_count as f64 + 16.0 + frame_count as f64 / 64.0) * core_rate / dest_rate;
        self.shared.high_water.store(high_water as u32, Ordering::Relaxed);

        let source = core.audio_buffer();
        self.resampler.set_source(source, core_rate, true);
        self.resampler.set_destination(&mut self.dest_buffer, dest_rate);
        self.resampler.process();
        drop(core);
        // Samples consumed — let the drive thread top the queue back up.
        self.shared.consumed.notify_all();

        let available = self.dest_buffer.available().min(frame_count);
        self.dest_buffer.read(
            &mut linear_buf[..available * crate::platform::audio::NUM_CHANNELS],
            available,
        );
        available
    }
}
