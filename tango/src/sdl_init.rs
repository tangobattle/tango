//! Central SDL3 initialization. SDL3's `Sdl` (and the `EventPump`
//! derived from it) are `!Send`, and the library enforces that init
//! happens on the main thread (via a thread-local check inside
//! `Sdl::new`), so we run [`init`] once from the iced/winit main
//! thread and stash the handles in [`send_wrapper::SendWrapper`]
//! globals. `SendWrapper` is `Send`/`Sync` but panics if the inner
//! value is touched from any thread other than the one that built
//! it, which is exactly the main-thread-only guarantee we need.
//! Callers grab subsystems (audio, gamepad, ...) via [`sdl`] and
//! drain input via [`event_pump`] — all of which must run on the
//! main thread.
//!
//! Lives in its own module so audio + gamepad don't both try to own
//! the SDL context; one canonical owner, multiple borrowers. The
//! `EventPump` is a singleton (the sdl3 crate ref-counts it, so only
//! one can exist at a time), which is the other reason it belongs
//! here rather than inside any single subsystem.

use std::ops::{Deref, DerefMut};
use std::sync::{Mutex, MutexGuard};

use sdl3::{EventPump, Sdl};
use send_wrapper::SendWrapper;

use crate::audio;

static SDL: Mutex<Option<SendWrapper<Sdl>>> = Mutex::new(None);
static EVENT_PUMP: Mutex<Option<SendWrapper<EventPump>>> = Mutex::new(None);

/// Initialize SDL3 once at startup on the main thread. Failures are
/// logged and turn [`sdl`] / [`event_pump`] into `None`-returning
/// no-ops — callers that depend on SDL (audio, gamepad) fall back to
/// silent / unavailable modes without taking the app down.
pub fn init() {
    // Per the SDL3 gamepad example: needed on Windows so the joystick
    // subsystem spins up its own polling thread when we don't have a
    // video subsystem hooked into the message loop.
    sdl3::hint::set("SDL_JOYSTICK_THREAD", "1");
    // Nudge SDL toward our preferred audio buffer size for low
    // playback latency. Advisory only — the driver can still pick a
    // larger quantum.
    sdl3::hint::set("SDL_AUDIO_DEVICE_SAMPLE_FRAMES", &audio::SAMPLES.to_string());
    sdl3::hint::set("SDL_APP_NAME", "Tango");
    sdl3::hint::set("SDL_WINDOWS_INTRESOURCE_ICON", "1");

    let sdl = match sdl3::init() {
        Ok(s) => s,
        Err(e) => {
            log::warn!("sdl3 init failed: {e}");
            return;
        }
    };
    // Grab the (singleton) event pump now so it lives centrally
    // alongside the `Sdl` handle.
    match sdl.event_pump() {
        Ok(pump) => *EVENT_PUMP.lock().unwrap() = Some(SendWrapper::new(pump)),
        Err(e) => log::warn!("sdl3 event pump init failed: {e}"),
    }
    *SDL.lock().unwrap() = Some(SendWrapper::new(sdl));
}

/// RAII borrow of the global [`Sdl`], returned by [`sdl`]. Deref to
/// reach the subsystems: `sdl.audio()`, `sdl.gamepad()`, ...
///
/// Holds a `MutexGuard`, which makes `SdlGuard` `!Send` for free —
/// so a borrow of the `Sdl` can't be smuggled off the thread it was
/// taken on. Hold it only as briefly as you need: it keeps the
/// global mutex locked, and calling [`sdl`] again while one is alive
/// deadlocks.
pub struct SdlGuard {
    guard: MutexGuard<'static, Option<SendWrapper<Sdl>>>,
}

impl Deref for SdlGuard {
    type Target = Sdl;
    fn deref(&self) -> &Sdl {
        // `sdl` only builds a guard when the global is `Some`, and we
        // hold the lock for the guard's whole life, so it can't have
        // been cleared. The `SendWrapper` deref panics if we're on
        // the wrong thread.
        self.guard.as_ref().unwrap()
    }
}

/// RAII exclusive borrow of the global [`EventPump`], returned by
/// [`event_pump`]. Deref-muts to poll events. Same `!Send`,
/// hold-it-briefly caveats as [`SdlGuard`].
pub struct EventPumpGuard {
    guard: MutexGuard<'static, Option<SendWrapper<EventPump>>>,
}

impl Deref for EventPumpGuard {
    type Target = EventPump;
    fn deref(&self) -> &EventPump {
        self.guard.as_ref().unwrap()
    }
}

impl DerefMut for EventPumpGuard {
    fn deref_mut(&mut self) -> &mut EventPump {
        self.guard.as_mut().unwrap()
    }
}

/// Borrow the global [`Sdl`]. Returns `None` if [`init`] never
/// succeeded. The returned guard panics on deref if used from a
/// thread other than the one that ran [`init`] (via `SendWrapper`).
pub fn sdl() -> Option<SdlGuard> {
    let guard = SDL.lock().unwrap();
    guard.as_ref()?;
    Some(SdlGuard { guard })
}

/// Borrow the global [`EventPump`] for draining input. Returns
/// `None` if [`init`] never succeeded or the pump failed to start.
/// Same wrong-thread panic-on-deref behavior as [`sdl`].
pub fn event_pump() -> Option<EventPumpGuard> {
    let guard = EVENT_PUMP.lock().unwrap();
    guard.as_ref()?;
    Some(EventPumpGuard { guard })
}
