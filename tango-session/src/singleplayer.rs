//! Standalone (no-netplay) emulator session: one machine on a link
//! with nobody on the other end. Boots a ROM with the user-selected
//! save and accepts joyflag input from the host's tick loop. The video
//! frame plumbing mirrors the other sessions — the driver writes mgba's
//! raw BGR555 into the session's own [`Framebuffer`](crate::Framebuffer)
//! (the framebuffer shader expands it to RGB on the GPU).
//!
//! It runs on a one-side [`Link`](tango_match::Link) rather than a bare
//! core, which is what every other session kind here runs on: the cart
//! sees its link hardware from power-on, the savedata comes back out
//! through [`Link::export_save`](tango_match::Link::export_save)
//! wherever the host wants to put it, and this is the machine a future
//! netplay handoff can plug a cable into.
//!
//! The core runs wherever the host drives it from (mgba is built
//! without its thread runner). [`Driver::tick`] is one emulated frame,
//! and the host decides what turns it: a desktop runs it on a thread of
//! its own, paced to [`Driver::fps_target`]; a browser calls it from the
//! event loop. Neither the thread nor the pacing lives here. Audio
//! follows as a
//! pure consumer through the shared
//! [`CoreStream`](crate::audio::CoreStream) rate control, so a
//! stalled or torn-down audio device costs sound, never the session.
//!
//! No hooks::Hooks traps are installed: this is a vanilla emulator
//! ride for one player. (The PVP / replay traps require a partner /
//! recorded packets, neither of which apply here.)

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

const EXPECTED_FPS: f32 = 60.0;

/// The session's machine, shared between whatever drives it and the
/// audio stream (which pulls samples off it between ticks).
type SharedLink = Arc<Mutex<tango_match::Link>>;

/// Audio pull over the session's mutex — a driver holds it only while
/// stepping a frame, so the readout interleaves between ticks.
/// Uncontended on a single-threaded host; the lock still matters there,
/// because re-entering it from inside a tick would deadlock.
struct SharedLinkPull(SharedLink);

impl crate::audio::PairPull for SharedLinkPull {
    fn with_pair(&self, f: &mut dyn FnMut(&mut tango_match::Link)) {
        f(&mut self.0.lock().unwrap());
    }
}

pub struct SinglePlayerSession {
    game: &'static tango_gamesupport::Game,
    link: SharedLink,
    joyflags: Arc<AtomicU32>,
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
        sample_rate: u32,
    ) -> Result<(Self, Driver, crate::audio::CoreStream), crate::Error> {
        let mut link = tango_match::Link::with_options(tango_match::LinkOptions {
            sides: vec![tango_match::SideOptions {
                rom: rom.as_ref().clone(),
                save,
            }],
            rtc,
            peripheral: tango_match::Peripheral::Cable,
        })?;
        // Queue headroom for the stream's rate control — the discard cap
        // sits at 3x its 50 ms target and fast-forward piles up several
        // callbacks' worth between fills; mGBA's default buffer doesn't
        // hold that at BN4+'s 65536 Hz. Same sizing as the pair engine.
        link.core_mut(0).set_audio_buffer_size(16384);
        link.core_mut(0).audio_buffer().clear();

        let link: SharedLink = Arc::new(Mutex::new(link));
        let joyflags = Arc::new(AtomicU32::new(0));
        let fps_bits = Arc::new(AtomicU32::new(EXPECTED_FPS.to_bits()));
        let stop = Arc::new(AtomicBool::new(false));
        let screen = crate::Framebuffer::new();
        let wake = Arc::new(tokio::sync::Notify::new());

        let driver = Driver {
            link: link.clone(),
            joyflags: joyflags.clone(),
            fps_bits: fps_bits.clone(),
            stop: stop.clone(),
            screen: screen.clone(),
            wake: wake.clone(),
        };
        let audio = crate::audio::CoreStream::new(
            crate::audio::PairCorePull {
                pair: SharedLinkPull(link.clone()),
                player: Box::new(|| 0),
            },
            crate::audio::CoreStream::fps_from_bits(fps_bits.clone()),
            sample_rate,
        );

        Ok((
            Self {
                game,
                link,
                joyflags,
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
        self.link.lock().unwrap().export_save(0)
    }
}

impl crate::Session for SinglePlayerSession {
    fn local_game(&self) -> &'static tango_gamesupport::Game {
        self.game
    }

    fn frame(&self) -> Vec<u8> {
        self.screen.read()
    }

    fn wake(&self) -> Arc<tokio::sync::Notify> {
        self.wake.clone()
    }

    fn set_joyflags(&self, joyflags: u32) {
        self.joyflags.store(joyflags, Ordering::Relaxed);
    }

    fn set_speed(&self, factor: f32) {
        self.fps_bits
            .store(crate::clamp_speed(EXPECTED_FPS, factor).to_bits(), Ordering::Relaxed);
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
    link: SharedLink,
    joyflags: Arc<AtomicU32>,
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
        {
            // Scoped: the audio pull takes this same mutex, so it must be
            // free between ticks (and, on a single-threaded host, before
            // this call returns to the pump).
            let mut link = self.link.lock().unwrap();
            if let Err(e) = link.try_tick(&[self.joyflags.load(Ordering::Relaxed)]) {
                log::error!("single-player emulation failed: {e}");
                self.stop.store(true, Ordering::Relaxed);
                return false;
            }
            if let Some(frame) = link.video_buffer(0) {
                // mgba's native BGR555 goes up as-is; the framebuffer
                // shader expands it to RGB on the GPU at draw time.
                self.screen.write(frame);
            }
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
