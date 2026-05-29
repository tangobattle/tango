//! Central SDL3 initialization. SDL3's `Sdl` is `!Send` and the
//! library enforces that init happens on the main thread (via a
//! thread-local check inside `Sdl::new`), so we run [`init`] once
//! from the iced/winit main thread and stash the handle in a
//! `Send`-newtype'd global. Callers grab subsystems (audio,
//! gamepad, event pump, ...) via [`with_sdl`] — all of which
//! must also run on the main thread.
//!
//! Lives in its own module so audio + gamepad don't both try to
//! own the SDL context; one canonical owner, multiple borrowers.

use std::thread::ThreadId;

use parking_lot::Mutex;
use sdl3::Sdl;

use crate::audio;

struct SendSdl {
    sdl: Sdl,
    owner: ThreadId,
}

/// SAFETY: [`with_sdl`] panics if accessed from any thread other
/// than the one that constructed the [`SendSdl`], so the `!Send`
/// `Sdl` is only ever touched on its owning thread despite the
/// wrapper itself being `Send`-able into the global `Mutex`.
unsafe impl Send for SendSdl {}

static SDL: Mutex<Option<SendSdl>> = Mutex::new(None);

/// Initialize SDL3 once at startup on the main thread. Failures
/// are logged and turn [`with_sdl`] into a `None`-returning
/// no-op — callers that depend on SDL (audio, gamepad) fall back
/// to silent / unavailable modes without taking the app down.
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
    *SDL.lock() = Some(SendSdl {
        sdl,
        owner: std::thread::current().id(),
    });
}

/// Run `f` with a borrow of the global `Sdl`. Returns `None` if
/// [`init`] never succeeded. Panics if called from a thread other
/// than the one that ran [`init`] — sdl3's own checks would also
/// catch this, but a clear panic message helps.
pub fn with_sdl<R>(f: impl FnOnce(&Sdl) -> R) -> Option<R> {
    let guard = SDL.lock();
    let s = guard.as_ref()?;
    let cur = std::thread::current().id();
    assert_eq!(
        cur, s.owner,
        "sdl context accessed from thread {cur:?} but was initialized on {:?}",
        s.owner,
    );
    Some(f(&s.sdl))
}
