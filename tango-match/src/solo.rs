//! Running one console on its own.
//!
//! Netplay is not the only thing a host asks an engine for: it also
//! wants a single machine for the save-editor's "just play it" ride.
//! That is the same shape as a match — boot a console, feed it input,
//! take frames and audio off it — and the solo ride takes the same
//! shape [`Match`](crate::Match) does: the engine contributes only a
//! booted [`Console`], and [`Solo`] — one concrete type, here — is the
//! whole ride over it.
//!
//! Optional: a game that only supports netplay implements none of it,
//! and the host reports that the ride isn't available rather than
//! failing to build.

/// One console booted alone, as an engine hands it to the seam.
///
/// The counterpart of [`Link`](crate::Link) for a machine with no
/// pair: it ticks, and it has a [`side`](crate::Side). Rollback has no
/// seat here — there is no peer to mispredict — so neither do
/// snapshots, sanitizing, or a second input.
pub trait Console: Send + 'static {
    /// Advance one video frame with the input held this tick — the
    /// joypad bits (see [`keys`](crate::keys)) plus the stylus, which
    /// only a touch-screen console reads. The engine reduces it to
    /// what the console can express, exactly as a link sanitizes. An
    /// error ends the ride — a corrupt core stops the session rather
    /// than panicking the host.
    fn tick(&mut self, input: crate::HostInput) -> Result<(), crate::Error>;

    /// The console's per-side surface: display, audio out, savedata.
    fn side(&mut self) -> Box<dyn crate::Side + '_>;
}

/// One console running by itself, as a host drives it — the solo
/// counterpart of [`Match`](crate::Match), and like it the one
/// concrete type: every engine's ride is this struct over that
/// engine's [`Console`], so the ride's plumbing exists once, here.
///
/// Clone-shared: the handle a host's drive loop ticks and the handle
/// its session reads the save from are the same console behind one
/// lock.
#[derive(Clone)]
pub struct Solo {
    console: std::sync::Arc<std::sync::Mutex<dyn Console>>,
}

impl Solo {
    /// Take the ride over a booted console.
    pub fn new(console: impl Console) -> Self {
        Solo {
            console: std::sync::Arc::new(std::sync::Mutex::new(console)),
        }
    }

    /// Advance one video frame with the input held this tick. An error
    /// ends the ride.
    pub fn tick(&self, input: crate::HostInput) -> Result<(), crate::Error> {
        self.console.lock().unwrap().tick(input)
    }

    /// The console's display, RGBA8 in
    /// [`screen_layout`](crate::Backend::screen_layout) order.
    /// `None` before its first frame.
    pub fn frame(&self) -> Option<Vec<u8>> {
        self.console.lock().unwrap().side().frame()
    }

    /// The cartridge's savedata as it stands, or `None` for a game that
    /// has never written any. The host owns persisting it.
    pub fn export_save(&self) -> Option<Vec<u8>> {
        self.console.lock().unwrap().side().export_save()
    }

    /// A handle onto this console, for a host reading it off the thread
    /// that ticks it — its audio, mainly.
    pub fn side_source(&self) -> Box<dyn crate::SideSource> {
        Box::new(LoneConsole {
            console: self.console.clone(),
        })
    }
}

/// A console booted alone, as [`Solo::side_source`] hands it out:
/// behind the same lock the ride ticks it under.
struct LoneConsole {
    console: std::sync::Arc<std::sync::Mutex<dyn Console>>,
}

impl crate::SideSource for LoneConsole {
    fn with_side(&self, f: &mut dyn FnMut(&mut dyn crate::Side)) {
        f(&mut *self.console.lock().unwrap().side());
    }

    fn try_side(&self, f: &mut dyn FnMut(&mut dyn crate::Side)) -> bool {
        match self.console.try_lock() {
            Ok(mut console) => {
                f(&mut *console.side());
                true
            }
            Err(_) => false,
        }
    }
}

/// Everything a solo ride needs to come up.
pub struct SoloConfig<'a> {
    /// The cartridge image, already patched.
    pub rom: &'a [u8],
    /// Save memory, or `None` for a blank cart.
    pub save: Option<&'a [u8]>,
    /// Pins the cart clock. `None` leaves it on the real one, which is
    /// what a desktop wants and what a browser — where there is no such
    /// clock to read — must fill in.
    pub rtc: Option<std::time::SystemTime>,
}
