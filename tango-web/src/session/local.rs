//! A solo session: one machine on a cable link with nothing on the
//! other end, driven straight by the pump (no rollback engine).
//!
//! Boot goes through the library: the caller identifies the ROM with
//! `library::detect` and hands the registration over, so the session
//! knows its game (save parsing, display strings, and — for PvP — the
//! gamesupport hooks all key off it).

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use mgba_siolink::{Link, LinkOptions, SideOptions};

use crate::library::GameRef;
use crate::session::{
    prepare_audio_buffers, LinkAccess, SessionDescriptor, SessionEnd, SessionKind, SharedSession,
    EXPECTED_FPS,
};

pub struct LocalArgs {
    /// The registered game `rom` was identified as.
    pub game: GameRef,
    pub rom: Vec<u8>,
    /// Save data for the solo cart.
    pub save: Option<Vec<u8>>,
    /// The cart clock's boot value. `SystemTime::now()` panics on wasm,
    /// so the host supplies it (from `js_sys::Date::now()`).
    pub rtc: std::time::SystemTime,
}

/// A booted local session: the driver the runtime pump ticks, plus the
/// shared state and link access the presenter/audio/UI hang off.
pub struct LocalSession {
    pub driver: LocalDriver,
    pub shared: Arc<SharedSession>,
    pub link: LinkAccess,
    pub descriptor: SessionDescriptor,
}

/// Boot a fresh local session from power-on.
pub fn start(args: LocalArgs) -> anyhow::Result<LocalSession> {
    let mut link = Link::with_options(LinkOptions {
        sides: vec![SideOptions {
            rom: args.rom,
            save: args.save,
        }],
        rtc: Some(args.rtc),
        peripheral: mgba_siolink::Peripheral::Cable,
    })?;
    prepare_audio_buffers(&mut link);
    let link = Arc::new(Mutex::new(link));

    let shared = SharedSession::new(0);
    let descriptor = SessionDescriptor {
        kind: SessionKind::Local,
        local_player: 0,
        game: args.game,
    };

    Ok(LocalSession {
        driver: LocalDriver::new(shared.clone(), link.clone()),
        shared,
        link: LinkAccess::Shared(link),
        descriptor,
    })
}

/// The solo session's per-tick body. The pump owns pacing and the pause
/// gate; `tick` assumes it is only called when the session should
/// actually advance one frame.
pub struct LocalDriver {
    shared: Arc<SharedSession>,
    link: Arc<Mutex<Link>>,
}

impl LocalDriver {
    pub fn new(shared: Arc<SharedSession>, link: Arc<Mutex<Link>>) -> LocalDriver {
        LocalDriver { shared, link }
    }

    /// Advance one frame. Returns `false` once the session has ended
    /// (the end is already recorded in `shared`).
    pub fn tick(&mut self) -> bool {
        if self.shared.quit.load(Ordering::Relaxed) {
            self.shared.finish(SessionEnd::LocalQuit);
            return false;
        }

        let joyflags = self.shared.joyflags.load(Ordering::Relaxed) & 0x3ff;

        {
            let mut link = self.link.lock().unwrap();
            // A corrupt link state must end the session with a message,
            // not panic the app into a frozen tab.
            if let Err(e) = link.try_tick(&[joyflags]) {
                drop(link);
                self.shared
                    .finish(SessionEnd::Error(format!("emulation error: {e}")));
                return false;
            }
            if let Some(buf) = link.video_buffer(0) {
                self.shared.publish_video(buf);
            }
        }

        // Hold-to-fast-forward comes in via the speed knob.
        let speed = self.shared.speed.load(Ordering::Relaxed).max(25) as f32 / 100.0;
        let fps_target = EXPECTED_FPS * speed;
        self.shared.set_fps_target(fps_target);
        {
            let mut stats = self.shared.stats.lock().unwrap();
            stats.fps_target = fps_target;
            stats.frontier += 1;
        }
        true
    }
}
