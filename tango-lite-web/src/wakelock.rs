//! Keeping the screen on while a session runs.
//!
//! A phone dims and locks after a handful of idle seconds, and to the
//! OS a player holding still through a custom screen is idle — nothing
//! here is touch input. In a link battle that is worse than an
//! annoyance: the screen locking backgrounds the page, and a stalled
//! simulation is not a local problem, it backs the peer's input queue
//! up until their supervisor gives the link up for dead.
//!
//! Reached through `Reflect` rather than `web_sys::WakeLock`, which is
//! behind the `web_sys_unstable_apis` cfg — a global `RUSTFLAGS` knob
//! this build would have to carry everywhere to use one API. Absent
//! entirely on some browsers, which is why every step here degrades to
//! doing nothing.

use std::cell::{Cell, RefCell};

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

thread_local! {
    /// The live `WakeLockSentinel`, if we hold one.
    static SENTINEL: RefCell<Option<JsValue>> = const { RefCell::new(None) };
    /// Whether a session wants the screen kept awake. Separate from
    /// holding one, because the request is async and the browser drops
    /// the lock whenever the page is hidden.
    static WANTED: Cell<bool> = const { Cell::new(false) };
    static WATCHING: Cell<bool> = const { Cell::new(false) };
}

/// Keep the screen on until [`release`]. Idempotent.
pub fn hold() {
    WANTED.with(|w| w.set(true));
    watch_visibility();
    request();
}

/// Let the screen sleep again. Idempotent.
pub fn release() {
    WANTED.with(|w| w.set(false));
    if let Some(sentinel) = SENTINEL.with(|s| s.borrow_mut().take()) {
        drop_sentinel(&sentinel);
    }
}

/// Ask for the lock, unless one is already held or nobody wants one.
fn request() {
    if !WANTED.with(|w| w.get()) || SENTINEL.with(|s| s.borrow().is_some()) {
        return;
    }
    let Some(promise) = call_request() else { return };
    wasm_bindgen_futures::spawn_local(async move {
        match JsFuture::from(promise).await {
            Ok(sentinel) => {
                // The session may have ended while the request was in
                // flight; a lock nobody wants is one that never gets
                // released.
                if WANTED.with(|w| w.get()) {
                    SENTINEL.with(|s| *s.borrow_mut() = Some(sentinel));
                } else {
                    drop_sentinel(&sentinel);
                }
            }
            // Refused for any of the ordinary reasons — no permission,
            // low battery, not a top-level document. The session runs
            // regardless; it just doesn't get to hold the screen.
            Err(e) => log::debug!("wake lock refused: {e:?}"),
        }
    });
}

/// `navigator.wakeLock.request("screen")`, or `None` where there is no
/// such thing.
fn call_request() -> Option<js_sys::Promise> {
    let navigator: JsValue = web_sys::window()?.navigator().into();
    let wake_lock = js_sys::Reflect::get(&navigator, &"wakeLock".into()).ok()?;
    if wake_lock.is_undefined() || wake_lock.is_null() {
        return None;
    }
    let request = js_sys::Reflect::get(&wake_lock, &"request".into())
        .ok()?
        .dyn_into::<js_sys::Function>()
        .ok()?;
    request
        .call1(&wake_lock, &"screen".into())
        .ok()?
        .dyn_into::<js_sys::Promise>()
        .ok()
}

fn drop_sentinel(sentinel: &JsValue) {
    let Ok(release) = js_sys::Reflect::get(sentinel, &"release".into()) else {
        return;
    };
    if let Ok(release) = release.dyn_into::<js_sys::Function>() {
        let _ = release.call0(sentinel);
    }
}

/// The browser releases the lock every time the page is hidden and will
/// not hand it back on its own, so coming back to a still-running
/// session has to ask again.
fn watch_visibility() {
    if WATCHING.with(|w| w.replace(true)) {
        return;
    }
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    let on_change = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
        let visible = web_sys::window()
            .and_then(|window| window.document())
            .map(|document| document.visibility_state() == web_sys::VisibilityState::Visible)
            .unwrap_or(false);
        if visible {
            // Whatever we were holding is already gone; forget it before
            // asking, or the guard in `request` sees a dead sentinel and
            // declines.
            SENTINEL.with(|s| *s.borrow_mut() = None);
            request();
        }
    });
    let _ = document
        .add_event_listener_with_callback("visibilitychange", on_change.as_ref().unchecked_ref());
    // Page-lifetime listener: leaking it is how you say "never removed".
    on_change.forget();
}
