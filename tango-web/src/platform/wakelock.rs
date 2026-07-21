//! Screen Wake Lock glue: hold `navigator.wakeLock` while a session is
//! live so the screen doesn't dim or sleep mid-game — touch play and
//! cutscenes can go minutes without an input the OS would count as
//! activity. The browser silently releases the lock whenever the page
//! hides (and on battery-saver whims), so [`install`]'s visibility hook
//! re-requests it when the page comes back with a session still live.
//! Unsupported browsers (insecure contexts, pre-16.4 iOS) just no-op:
//! the lock is a comfort, never a dependency.

use std::cell::{Cell, RefCell};

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

// web-sys's WakeLock bindings are still gated behind
// --cfg=web_sys_unstable_apis; a hand-rolled extern keeps that
// workspace-wide flag out of the build.
#[wasm_bindgen]
extern "C" {
    type WakeLock;
    #[wasm_bindgen(method)]
    fn request(this: &WakeLock, kind: &str) -> js_sys::Promise;

    type WakeLockSentinel;
    #[wasm_bindgen(method)]
    fn release(this: &WakeLockSentinel) -> js_sys::Promise;
}

thread_local! {
    /// Whether a live session wants the screen held awake.
    static WANTED: Cell<bool> = const { Cell::new(false) };
    /// A request is already in flight; don't stack another.
    static REQUESTING: Cell<bool> = const { Cell::new(false) };
    /// The held sentinel. May be stale (auto-released by the browser);
    /// releasing a released sentinel is a resolved no-op, so staleness
    /// only ever costs a redundant release call.
    static SENTINEL: RefCell<Option<WakeLockSentinel>> = const { RefCell::new(None) };
}

/// `navigator.wakeLock`, where the browser has one (secure contexts on
/// Chrome 84+ / Safari 16.4+ / Firefox 126+).
fn wake_lock() -> Option<WakeLock> {
    let nav = web_sys::window()?.navigator();
    let wl = js_sys::Reflect::get(nav.as_ref(), &"wakeLock".into()).ok()?;
    (!wl.is_undefined()).then(|| wl.unchecked_into())
}

fn drop_sentinel() {
    if let Some(sentinel) = SENTINEL.with(|s| s.borrow_mut().take()) {
        let _ = sentinel.release();
    }
}

fn request() {
    if REQUESTING.get() || SENTINEL.with(|s| s.borrow().is_some()) {
        return;
    }
    let Some(lock) = wake_lock() else { return };
    REQUESTING.set(true);
    wasm_bindgen_futures::spawn_local(async move {
        let result = wasm_bindgen_futures::JsFuture::from(lock.request("screen")).await;
        REQUESTING.set(false);
        match result {
            Ok(sentinel) => {
                let sentinel: WakeLockSentinel = sentinel.unchecked_into();
                if WANTED.get() {
                    SENTINEL.with(|s| *s.borrow_mut() = Some(sentinel));
                } else {
                    // The session ended while the request was in flight.
                    let _ = sentinel.release();
                }
            }
            // NotAllowedError: hidden document or battery saver. The
            // visibility hook retries when the page comes back.
            Err(e) => log::debug!("wake lock unavailable: {e:?}"),
        }
    });
}

/// A session started or ended; hold or drop the lock to match.
pub fn set_active(active: bool) {
    WANTED.set(active);
    if active {
        request();
    } else {
        drop_sentinel();
    }
}

/// Install the visibility hook (idempotent per page load only because
/// [`crate::runtime::Runtime::install`] is). Logs support once so an
/// always-dimming screen is explicable from the console.
pub fn install() {
    if wake_lock().is_none() {
        log::info!("screen wake lock unsupported; the screen may dim mid-game");
        return;
    }
    let document = web_sys::window().unwrap().document().unwrap();
    let closure: Closure<dyn FnMut(web_sys::Event)> = {
        let document = document.clone();
        Closure::new(move |_| {
            if !document.hidden() && WANTED.get() {
                // Hiding auto-released the lock; the stored sentinel is
                // stale and request() would see it as already-held.
                drop_sentinel();
                request();
            }
        })
    };
    document
        .add_event_listener_with_callback("visibilitychange", closure.as_ref().unchecked_ref())
        .expect("addEventListener");
    // App-lifetime listener: leak deliberately.
    closure.forget();
}
