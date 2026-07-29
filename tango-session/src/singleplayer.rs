//! Standalone (no-netplay) emulator session: one machine on a link
//! with nobody on the other end. Boots a ROM with the user-selected
//! save and accepts joyflag input from the host's tick loop. The video
//! frame plumbing mirrors the other sessions — the driver publishes the
//! console's frame into the session's own
//! [`Framebuffer`](crate::Framebuffer).
//!
//! The console comes from the game's own registration
//! ([`start_solo`](tango_match::Backend::start_solo)), so this
//! session never learns which emulator is underneath and a game whose
//! engine offers no single-player ride simply says so.
//!
//! The console runs wherever the host drives it from. [`Driver::tick`]
//! is one emulated frame,
//! and the host decides what turns it: a desktop runs it on a thread of
//! its own, paced to [`Driver::fps_target`]; a browser calls it from the
//! event loop. Neither the thread nor the pacing lives here. Audio
//! follows as a
//! pure consumer through the shared
//! [`CoreStream`](crate::audio::CoreStream) rate control, so a
//! stalled or torn-down audio device costs sound, never the session.
//!
//! No priming happens: this is a vanilla ride for one player, where
//! netplay's traps would have nothing to prime towards.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use crate::InputCell;

pub struct SinglePlayerSession {
    game: &'static tango_gamesupport::Game,
    /// The seam's solo ride, clone-shared with whatever drives it (the
    /// session reads the save off its own handle).
    console: tango_match::Solo,
    layout: tango_match::ScreenLayout,
    input: Arc<InputCell>,
    /// The engine's native frame rate — what the speed dial's 1.0× means.
    expected_fps: f32,
    /// Pacing target as f32 bits. 60.0 = realtime; fast-forward raises it
    /// and the audio stream's faux clock compresses to match.
    fps_bits: Arc<AtomicU32>,
    stop: Arc<AtomicBool>,
    screen: Arc<crate::Framebuffer>,
    wake: Arc<tokio::sync::Notify>,
}

impl SinglePlayerSession {
    /// Boot the session. Also returns the driver the host will tick and
    /// the session's audio stream — its samples resampled to
    /// `sample_rate` — for the host to route to its output; dropping the
    /// stream just costs sound (pacing doesn't depend on the audio
    /// device at all).
    ///
    /// `save` is the cartridge's savedata image, or `None` for a cart
    /// that starts blank; the game's writes land in memory and the host
    /// persists them from [`export_save`](Self::export_save). `rtc`
    /// pins the cart clock — `None` leaves it on the real one, which is
    /// what a desktop wants and what a browser (where there is no such
    /// clock to read) must fill in.
    pub fn new(
        game: &'static tango_gamesupport::Game,
        rom: Arc<Vec<u8>>,
        save: Option<Vec<u8>>,
        rtc: Option<std::time::SystemTime>,
        expected_fps: f32,
        sample_rate: u32,
    ) -> Result<(Self, Driver, crate::audio::CoreStream), crate::Error> {
        let console = game.pvp.start_solo(tango_match::SoloConfig {
            rom: rom.as_ref(),
            save: save.as_deref(),
            rtc,
        })?;
        let audio_pull = console.audio();

        let layout = game.pvp.screen_layout();
        let input = InputCell::new();
        let fps_bits = Arc::new(AtomicU32::new(expected_fps.to_bits()));
        let stop = Arc::new(AtomicBool::new(false));
        let screen = crate::Framebuffer::new(&layout);
        let wake = Arc::new(tokio::sync::Notify::new());

        let driver = Driver {
            console: console.clone(),
            input: input.clone(),
            fps_bits: fps_bits.clone(),
            stop: stop.clone(),
            screen: screen.clone(),
            wake: wake.clone(),
        };
        let audio = crate::audio::CoreStream::new(
            audio_pull,
            expected_fps,
            crate::audio::CoreStream::fps_from_bits(fps_bits.clone()),
            sample_rate,
        );

        Ok((
            Self {
                game,
                console,
                layout,
                input,
                expected_fps,
                fps_bits,
                stop,
                screen,
                wake,
            },
            driver,
            audio,
        ))
    }

    /// The cartridge's savedata as it stands right now, or `None` for a
    /// game that has never written any. The host owns persisting it —
    /// nothing here writes files — so a desktop host should also take a
    /// copy periodically rather than only at teardown.
    pub fn export_save(&self) -> Option<Vec<u8>> {
        self.console.export_save()
    }
}

impl crate::Session for SinglePlayerSession {
    fn local_game(&self) -> &'static tango_gamesupport::Game {
        self.game
    }

    fn frame(&self) -> Vec<u8> {
        self.screen.read()
    }

    fn screen_layout(&self) -> tango_match::ScreenLayout {
        self.layout.clone()
    }

    fn wake(&self) -> Arc<tokio::sync::Notify> {
        self.wake.clone()
    }

    fn set_input(&self, input: crate::HostInput) {
        self.input.store(input);
    }

    fn set_speed(&self, factor: f32) {
        self.fps_bits.store(
            crate::clamp_speed(self.expected_fps, factor).to_bits(),
            Ordering::Relaxed,
        );
    }
}

impl Drop for SinglePlayerSession {
    /// Tell whoever is driving to stop. A host running the driver on a
    /// thread joins it itself; the next `tick` there returns false.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The session's emulation step, and everything it needs for the
/// session's life — each an `Arc` shared with the session (and, for the
/// machine, the audio pull). Whoever holds this turns the crank: a
/// drive thread on the desktop, the event loop in a browser.
pub struct Driver {
    console: tango_match::Solo,
    input: Arc<InputCell>,
    fps_bits: Arc<AtomicU32>,
    stop: Arc<AtomicBool>,
    screen: Arc<crate::Framebuffer>,
    wake: Arc<tokio::sync::Notify>,
}

impl crate::Drive for Driver {
    fn tick(&mut self) -> bool {
        Driver::tick(self)
    }

    fn fps_target(&self) -> f32 {
        Driver::fps_target(self)
    }
}

impl Driver {
    /// Run one emulated frame and publish it. `false` once the session
    /// has been dropped, or once emulation has failed — a corrupt core
    /// ends the session rather than panicking the host.
    pub fn tick(&self) -> bool {
        if self.stop.load(Ordering::Relaxed) {
            return false;
        }
        if let Err(e) = self.console.tick(self.input.load()) {
            log::error!("single-player emulation failed: {e}");
            self.stop.store(true, Ordering::Relaxed);
            return false;
        }
        if let Some(frame) = self.console.frame() {
            self.screen.write(&frame);
        }
        // Wake the host's frame subscription so the UI rebuilds the
        // texture for this frame. Notify coalesces — a slow UI doesn't
        // queue up wakes.
        self.wake.notify_one();
        true
    }

    /// The session's current pacing target in fps — what a host paces
    /// `tick` to, and what the audio stream's faux clock follows.
    pub fn fps_target(&self) -> f32 {
        f32::from_bits(self.fps_bits.load(Ordering::Relaxed))
    }
}
