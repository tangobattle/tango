//! Central SDL3 initialization. SDL3's `Sdl` is `!Send` and the
//! library enforces that init happens on the main thread (via a
//! thread-local check inside `Sdl::new`), so we run [`init`] once
//! from the iced/winit main thread and stash the handle in a
//! `Send`-newtype'd global. Callers grab subsystems (audio,
//! gamepad, event pump, ...) via [`sdl`] — all of which
//! must also run on the main thread.
//!
//! Lives in its own module so audio + gamepad don't both try to
//! own the SDL context; one canonical owner, multiple borrowers.

use std::thread::ThreadId;

use std::sync::{Mutex, MutexGuard};
use sdl3::Sdl;

use crate::audio;

struct SendSdl {
    sdl: Sdl,
    owner: ThreadId,
}

/// SAFETY: [`sdl`] panics if accessed from any thread other than
/// the one that constructed the [`SendSdl`], so the `!Send` `Sdl`
/// is only ever touched on its owning thread despite the wrapper
/// itself being `Send`-able into the global `Mutex`.
unsafe impl Send for SendSdl {}

static SDL: Mutex<Option<SendSdl>> = Mutex::new(None);

/// Initialize SDL3 once at startup on the main thread. Failures
/// are logged and turn [`sdl`] into a `None`-returning no-op —
/// callers that depend on SDL (audio, gamepad) fall back to
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
    *SDL.lock().unwrap() = Some(SendSdl {
        sdl,
        owner: std::thread::current().id(),
    });
}

/// RAII borrow of the global [`Sdl`], returned by [`sdl`]. Deref
/// to reach the subsystems: `sdl.audio()`, `sdl.gamepad()`,
/// `sdl.event_pump()`, ...
///
/// Holds a `MutexGuard`, which makes `SdlGuard` `!Send` for free —
/// so a borrow of the `Sdl` can't be smuggled off the thread it
/// was taken on (the same thread [`init`] ran on). Hold it only as
/// briefly as you need: it keeps the global mutex locked, and
/// calling [`sdl`] again while one is alive deadlocks.
pub struct SdlGuard {
    guard: MutexGuard<'static, Option<SendSdl>>,
}

impl std::ops::Deref for SdlGuard {
    type Target = Sdl;
    fn deref(&self) -> &Sdl {
        // `sdl` only builds a guard when the global is `Some`, and
        // we hold the lock for the guard's whole life, so it can't
        // have been cleared out from under us.
        &self.guard.as_ref().unwrap().sdl
    }
}

/// Borrow the global [`Sdl`]. Returns `None` if [`init`] never
/// succeeded. Panics if called from a thread other than the one
/// that ran [`init`] — sdl3's own checks would also catch this,
/// but a clear panic message helps.
pub fn sdl() -> Option<SdlGuard> {
    let guard = SDL.lock().unwrap();
    let owner = guard.as_ref()?.owner;
    let cur = std::thread::current().id();
    assert_eq!(
        cur, owner,
        "sdl context accessed from thread {cur:?} but was initialized on {owner:?}",
    );
    Some(SdlGuard { guard })
}
