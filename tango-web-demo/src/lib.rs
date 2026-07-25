//! A browser host for [`tango_session`]: the smallest thing that proves
//! a session runs when a host pumps it instead of a thread driving it.
//!
//! The division of labour is the point. Everything emulator-shaped —
//! booting the machine, stepping frames, resampling audio, holding the
//! savedata — is the session crate's, unchanged from the desktop. This
//! crate is only the seam: it hands the page a session it can tick, a
//! framebuffer it can blit, samples it can queue, and the savedata to
//! persist. The page (`web/index.html`) owns the canvas, the keyboard,
//! the AudioWorklet and the pump, because those are a browser's
//! business and not a session's.
//!
//! Pumping is what a desktop drive thread would do, minus the sleeping:
//! `requestAnimationFrame` runs a slice of ticks per displayed frame,
//! and the worklet's queue reports run more when the tab is hidden and
//! rAF stops. Both call [`Demo::tick`].

#![cfg(target_arch = "wasm32")]

pub mod netplay;

use std::sync::LazyLock;

use tango_session::replay::ReplaySession;
use tango_session::singleplayer::SinglePlayerSession;
use tango_session::training::{NoopController, TrainingSession};
use tango_session::{Drive, Session};
use wasm_bindgen::prelude::*;

/// Every game the desktop app knows, so a dropped-in ROM resolves the
/// same way it does there.
static FAMILIES: LazyLock<Vec<&'static tango_gamesupport::Family>> = LazyLock::new(|| {
    let mut families: Vec<&'static tango_gamesupport::Family> = Vec::new();
    families.extend_from_slice(tango_gamesupport_bn1::FAMILIES);
    families.extend_from_slice(tango_gamesupport_bn2::FAMILIES);
    families.extend_from_slice(tango_gamesupport_bn3::FAMILIES);
    families.extend_from_slice(tango_gamesupport_bn4::FAMILIES);
    families.extend_from_slice(tango_gamesupport_bn5::FAMILIES);
    families.extend_from_slice(tango_gamesupport_bn6::FAMILIES);
    families.extend_from_slice(tango_gamesupport_exe45::FAMILIES);
    families
});

static GAMES: LazyLock<Vec<&'static tango_gamesupport::Game>> =
    LazyLock::new(|| tango_gamesupport::games_of(&FAMILIES));

/// The mgba C shim's clock, which a browser has to supply: wasm32 has
/// no `gettimeofday`, so mgba-sys leaves this symbol for the host to
/// define (savestate stamps and the cart RTC's default read it).
#[no_mangle]
pub extern "C" fn mgba_sys_now_unix_ms() -> f64 {
    js_sys::Date::now()
}

#[wasm_bindgen(start)]
pub fn init() {
    console_error_panic_hook::set_once();
    let _ = console_log::init_with_level(log::Level::Debug);
}

/// A booted session plus the scratch the page reads it through.
///
/// Which kind it is doesn't matter here: every kind publishes frames
/// through [`Session`] and advances through [`Drive`], so the page
/// drives a training battle exactly as it drives a solo cart.
#[wasm_bindgen]
pub struct Demo {
    session: Box<dyn Session>,
    driver: Box<dyn Drive>,
    /// A replay session's background pass, when that's what booted.
    /// Kept apart from the driver because the page schedules it: it
    /// shares this thread with playback, so it runs on leftovers.
    replay: Option<ReplayBackground>,
    audio: tango_session::audio::CoreStream,
    /// The last presented frame as RGBA, kept here so the page can view
    /// it in place rather than being handed a copy every frame.
    rgba: Vec<u8>,
    /// Scratch for one audio pull, reused the same way.
    samples: Vec<f32>,
}

#[wasm_bindgen]
impl Demo {
    /// Boot `rom` with `save` (the cartridge's savedata, or nothing for
    /// a blank cart) and an audio stream at `sample_rate`.
    ///
    /// `rtc_unix_secs` is the cart clock: a browser has no system clock
    /// a session may read, so the page passes `Date.now()`.
    pub fn boot(
        rom: Vec<u8>,
        save: Option<Vec<u8>>,
        sample_rate: u32,
        rtc_unix_secs: f64,
    ) -> Result<Demo, JsValue> {
        let game = tango_gamesupport::detect(&GAMES, &rom)
            .ok_or_else(|| JsValue::from_str("this ROM isn't a game Tango knows"))?;
        let rtc = std::time::UNIX_EPOCH + std::time::Duration::from_secs_f64(rtc_unix_secs.max(0.0));
        let (session, driver, audio) = SinglePlayerSession::new(
            game,
            std::sync::Arc::new(rom),
            save,
            Some(rtc),
            sample_rate,
        )
        .map_err(|e| JsValue::from_str(&format!("{e}")))?;
        Ok(Demo::wrap(Box::new(session), Box::new(driver), audio))
    }

    /// Play a recorded replay: the same re-simulation the desktop
    /// viewer runs, with its playhead, seek chases and prefetch pass
    /// interleaved on this one event loop.
    ///
    /// `roms` are the two sides' ROMs with any patch already applied,
    /// in player order — a browser has no ROM library to resolve them
    /// from, so the page supplies them.
    pub fn boot_replay(
        replay: Vec<u8>,
        p1_rom: Vec<u8>,
        p2_rom: Vec<u8>,
        sample_rate: u32,
    ) -> Result<Demo, JsValue> {
        let replay = tango_replay::Replay::decode(std::io::Cursor::new(replay))
            .map_err(|e| JsValue::from_str(&format!("this file isn't a replay Tango knows: {e}")))?;
        let games = [&p1_rom, &p2_rom].map(|rom| tango_gamesupport::detect(&GAMES, rom));
        let (Some(p1_game), Some(p2_game)) = (games[0], games[1]) else {
            return Err(JsValue::from_str("a side's ROM isn't a game Tango knows"));
        };
        let (session, workers, audio) = ReplaySession::new(
            [p1_game, p2_game],
            [std::sync::Arc::new(p1_rom), std::sync::Arc::new(p2_rom)],
            std::sync::Arc::new(replay),
            sample_rate,
            false,
            // No stats analysis: it exists to be cached, and there's
            // nowhere here to cache it.
            None,
        )
        .map_err(|e| JsValue::from_str(&format!("{e}")))?;
        let driver = std::rc::Rc::new(std::cell::RefCell::new(workers.into_driver()));
        Ok(Demo {
            session: Box::new(session),
            driver: Box::new(SharedDriver(driver.clone())),
            replay: Some(ReplayBackground(driver)),
            audio,
            rgba: vec![0u8; mgba_screen_pixels() * 4],
            samples: Vec::new(),
        })
    }

    /// Boot a training battle instead: the same cart against a
    /// do-nothing dummy on the other end of the link, primed into its
    /// link battle before this returns.
    ///
    /// `save` is a raw SRAM dump — training runs off it in memory and
    /// writes nothing back — and `seed` is the match seed a desktop
    /// would take from its RNG (a browser's is `crypto`).
    pub fn boot_training(
        rom: Vec<u8>,
        save: Vec<u8>,
        seed: Vec<u8>,
        sample_rate: u32,
        rtc_unix_secs: f64,
    ) -> Result<Demo, JsValue> {
        let game = tango_gamesupport::detect(&GAMES, &rom)
            .ok_or_else(|| JsValue::from_str("this ROM isn't a game Tango knows"))?;
        let mut rng_seed = [0u8; 16];
        for (slot, byte) in rng_seed.iter_mut().zip(seed) {
            *slot = byte;
        }
        let rtc = std::time::UNIX_EPOCH + std::time::Duration::from_secs_f64(rtc_unix_secs.max(0.0));
        let (session, driver, audio) = TrainingSession::new(
            game,
            std::sync::Arc::new(rom),
            save,
            rtc,
            rng_seed,
            sample_rate,
            Box::new(NoopController),
        )
        .map_err(|e| JsValue::from_str(&format!("{e}")))?;
        Ok(Demo::wrap(Box::new(session), Box::new(driver), audio))
    }

    pub(crate) fn wrap(
        session: Box<dyn Session>,
        driver: Box<dyn Drive>,
        audio: tango_session::audio::CoreStream,
    ) -> Demo {
        Demo {
            session,
            driver,
            replay: None,
            audio,
            rgba: vec![0u8; mgba_screen_pixels() * 4],
            samples: Vec::new(),
        }
    }

    /// Which game booted, for the page's status line.
    pub fn game(&self) -> String {
        let game = self.session.local_game();
        format!("{}-{}", game.family, game.variant)
    }

    /// Run up to `frames` emulated frames. Returns how many ran — fewer
    /// than asked means the session ended (a corrupt core), and the page
    /// should stop pumping.
    pub fn tick(&mut self, frames: u32) -> u32 {
        let mut ran = 0;
        for _ in 0..frames {
            if !self.driver.tick() {
                break;
            }
            ran += 1;
        }
        ran
    }

    /// Held buttons as an mgba joyflag bitmap; see [`Demo::key_bit`].
    pub fn set_keys(&mut self, keys: u32) {
        self.session.set_joyflags(keys);
    }

    /// The joyflag bit for a button name, so the page can keep its
    /// keyboard map in its own terms without hardcoding mgba's bits.
    /// Unknown names are 0, which is a no-op in the bitmap.
    pub fn key_bit(name: &str) -> u32 {
        use mgba_keys::*;
        match name {
            "a" => A,
            "b" => B,
            "l" => L,
            "r" => R,
            "start" => START,
            "select" => SELECT,
            "up" => UP,
            "down" => DOWN,
            "left" => LEFT,
            "right" => RIGHT,
            _ => 0,
        }
    }

    /// Convert the session's current frame to RGBA in place. Call
    /// before reading [`Demo::frame_ptr`].
    pub fn present(&mut self) {
        tango_dataview::rom::bgr555_to_rgba8(&self.session.frame(), &mut self.rgba);
    }

    /// Where the RGBA frame lives in wasm memory. The page builds a
    /// `Uint8ClampedArray` view over it — and must rebuild that view
    /// each frame, since growing the wasm heap detaches the old one.
    pub fn frame_ptr(&self) -> *const u8 {
        self.rgba.as_ptr()
    }

    pub fn frame_len(&self) -> usize {
        self.rgba.len()
    }

    pub fn screen_width() -> u32 {
        mgba::gba::SCREEN_WIDTH
    }

    pub fn screen_height() -> u32 {
        mgba::gba::SCREEN_HEIGHT
    }

    /// Pull `frames` stereo frames for the audio sink, interleaved and
    /// normalized to the [-1, 1] the Web Audio API wants. Short reads
    /// are normal — the session's rate control serves what it has, and
    /// silence while it has nothing.
    pub fn pull_audio(&mut self, frames: usize) -> Vec<f32> {
        use tango_session::audio::Stream as _;
        let mut buf = vec![[0i16; 2]; frames];
        let filled = self.audio.fill(&mut buf);
        self.samples.clear();
        self.samples
            .extend(buf[..filled].iter().flatten().map(|s| *s as f32 / 32768.0));
        self.samples.clone()
    }

    /// The cartridge's savedata as it stands, for the page to persist.
    /// `None` for a session that doesn't own one — a training battle
    /// runs off an in-memory dump and writes nothing back.
    pub fn export_save(&self) -> Option<Vec<u8>> {
        self.session
            .downcast_ref::<SinglePlayerSession>()
            .and_then(|s| s.export_save())
    }

    /// Jump a replay to `tick`. The chase runs inside the pump, a slice
    /// per frame, so the page keeps painting while it walks.
    pub fn seek(&self, tick: u32) {
        if let Some(replay) = self.session.downcast_ref::<ReplaySession>() {
            replay.seek_to(tick, false);
        }
    }

    /// Where a replay's playhead is, and how long it is — `None` for
    /// the session kinds that aren't a recording.
    pub fn playhead(&self) -> Option<Vec<u32>> {
        self.session
            .downcast_ref::<ReplaySession>()
            .map(|r| vec![r.current_tick(), r.total_ticks()])
    }

    /// Advance a replay's background prefetch by `budget` ticks —
    /// keyframes, so seeking backwards works. `false` once it's done
    /// (or this isn't a replay). The page calls this with whatever time
    /// the frame had left; too generous a budget and the emulator eats
    /// the event loop.
    pub fn prefetch(&mut self, budget: u32) -> bool {
        self.replay
            .as_mut()
            .is_some_and(|bg| bg.0.borrow_mut().prefetch_step(budget))
    }

    /// Whether the session has ended on its own — a training battle's
    /// match-end path, say. The page stops pumping when it has.
    pub fn is_ended(&self) -> bool {
        self.session.is_ended()
    }
}

/// The replay driver, shared between the [`Drive`] the page ticks and
/// the background pass it schedules separately. Single-threaded, so a
/// `RefCell` is the whole of the sharing — but the two borrows must
/// never overlap, which is why the page's pump and its idle work are
/// separate calls rather than one re-entrant one.
struct SharedDriver(std::rc::Rc<std::cell::RefCell<tango_session::replay::Driver>>);

impl Drive for SharedDriver {
    fn tick(&mut self) -> bool {
        self.0.borrow_mut().tick()
    }

    fn fps_target(&self) -> f32 {
        self.0.borrow().fps_target()
    }
}

struct ReplayBackground(std::rc::Rc<std::cell::RefCell<tango_session::replay::Driver>>);

/// mgba's joyflag bits, named for [`Demo::key_bit`].
mod mgba_keys {
    pub use mgba::input::keys::{A, B, DOWN, L, LEFT, R, RIGHT, SELECT, START, UP};
}

fn mgba_screen_pixels() -> usize {
    (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT) as usize
}
