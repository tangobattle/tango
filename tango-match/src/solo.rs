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
    inner: std::sync::Arc<std::sync::Mutex<Ride>>,
}

/// The console and where its sound goes, behind one lock — one lock
/// rather than two because the tick and the push that follows it are
/// the same critical section.
struct Ride {
    console: Box<dyn Console>,
    audio: Option<crate::audio::Pump>,
}

impl Solo {
    /// Take the ride over a booted console.
    ///
    /// `audio` is where its sound goes — the producing end of a
    /// [`channel`](crate::audio::channel) whose other end the host
    /// plays. `None` leaves the console holding its own audio, for a
    /// caller with nobody listening.
    pub fn new(console: impl Console, audio: Option<crate::AudioIn>) -> Self {
        Solo {
            inner: std::sync::Arc::new(std::sync::Mutex::new(Ride {
                console: Box::new(console),
                audio: audio.map(crate::audio::Pump::lone),
            })),
        }
    }

    /// Advance one video frame with the input held this tick. An error
    /// ends the ride.
    pub fn tick(&self, input: crate::HostInput) -> Result<(), crate::Error> {
        let mut guard = self.inner.lock().unwrap();
        let ride = &mut *guard;
        ride.console.tick(input)?;
        // With the console still in hand: what it just voiced crosses to
        // the host's device here, so a sound callback never has to reach
        // past this lock.
        if let Some(audio) = ride.audio.as_mut() {
            audio.pump_console(&mut *ride.console);
        }
        Ok(())
    }

    /// The console's display, RGBA8 in
    /// [`screen_layout`](crate::Backend::screen_layout) order.
    /// `None` before its first frame.
    pub fn frame(&self) -> Option<Vec<u8>> {
        self.inner.lock().unwrap().console.side().frame()
    }

    /// The cartridge's savedata as it stands, or `None` for a game that
    /// has never written any. The host owns persisting it.
    pub fn export_save(&self) -> Option<Vec<u8>> {
        self.inner.lock().unwrap().console.side().export_save()
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
    /// Where the console's sound goes — the producing end of a
    /// [`channel`](crate::audio::channel) whose other end the host
    /// plays. `None` for a caller with nobody listening.
    pub audio: Option<crate::AudioIn>,
}
