//! One mgba joyflag word, from touch and from a keyboard.
//!
//! The session takes the whole bitmap at once ([`Session::set_joyflags`]),
//! so both sources fold into one held-buttons word that the pump reads
//! each frame. Two sources rather than one because the same page is a
//! phone and a laptop; they simply OR together.
//!
//! [`Session::set_joyflags`]: tango_session::Session::set_joyflags

use std::cell::Cell;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use mgba::input::keys;

thread_local! {
    static TOUCH: Cell<u32> = const { Cell::new(0) };
    static KEYBOARD: Cell<u32> = const { Cell::new(0) };
}

/// Everything currently held, from either source.
pub fn joyflags() -> u32 {
    TOUCH.with(|t| t.get()) | KEYBOARD.with(|k| k.get())
}

/// Replace the bits under `mask` with `bits`.
///
/// Set rather than press/release because the D-pad is one control, not
/// four: a finger sliding across it goes from LEFT to LEFT|UP to UP
/// without ever lifting, and that is a single write of the whole
/// direction field. A plain button is the degenerate case —
/// `touch_set(A, A)` down, `touch_set(A, 0)` up.
pub fn touch_set(mask: u32, bits: u32) {
    TOUCH.with(|t| t.set((t.get() & !mask) | bits));
}

/// Drop everything held by touch. Used when the screen the pad lives on
/// goes away mid-press, which otherwise leaves a key stuck down.
pub fn touch_clear() {
    TOUCH.with(|t| t.set(0));
}

/// Every direction bit, as one field for [`touch_set`].
pub const DPAD: u32 = keys::UP | keys::DOWN | keys::LEFT | keys::RIGHT;

/// The desktop-shaped bindings, close enough to Tango's defaults to be
/// muscle-memory-compatible. Deliberately not configurable: a lite build
/// that needs a remapping screen isn't lite.
fn key_mask(code: &str) -> Option<u32> {
    Some(match code {
        "ArrowUp" | "KeyW" => keys::UP,
        "ArrowDown" | "KeyS" => keys::DOWN,
        "ArrowLeft" | "KeyA" => keys::LEFT,
        "ArrowRight" | "KeyD" => keys::RIGHT,
        "KeyZ" => keys::A,
        "KeyX" => keys::B,
        "KeyQ" => keys::L,
        "KeyE" => keys::R,
        "Enter" => keys::START,
        "Backspace" | "ShiftRight" | "ShiftLeft" => keys::SELECT,
        _ => return None,
    })
}

/// Whether this key event belongs to the emulator rather than to the
/// page.
///
/// Three ways it doesn't, and all three matter: half the button map is
/// ordinary letters (`wasdzxqe`) plus Backspace, so a listener that
/// takes them unconditionally makes every text field on the site
/// unusable — you cannot type a link code or a nickname, and you cannot
/// delete what you did manage to type. Which is exactly what happened.
fn is_for_the_game(event: &web_sys::KeyboardEvent) -> bool {
    // Nothing to play.
    if !crate::engine::is_running() {
        return false;
    }
    // A shortcut, not a button. (Shift is excluded from this test on
    // purpose — it's SELECT.)
    if event.ctrl_key() || event.meta_key() || event.alt_key() {
        return false;
    }
    // Someone is typing into something.
    let Some(target) = event.target() else { return true };
    match target.dyn_into::<web_sys::Element>() {
        Ok(element) => !matches!(element.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT"),
        Err(_) => true,
    }
}

/// Install the page-wide key listeners. Once, at startup: installing
/// them per screen would lose a key held across a screen change, and
/// they defer to the page unless [`is_for_the_game`] says otherwise.
pub fn install_keyboard() {
    let Some(window) = web_sys::window() else { return };

    let down = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(|e: web_sys::KeyboardEvent| {
        if !is_for_the_game(&e) {
            return;
        }
        // A repeat isn't a new press, and the arrows and Backspace
        // scroll or navigate away if we let them through.
        if let Some(mask) = key_mask(&e.code()) {
            e.prevent_default();
            if !e.repeat() {
                KEYBOARD.with(|k| k.set(k.get() | mask));
            }
        }
    });
    let up = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(|e: web_sys::KeyboardEvent| {
        // Deliberately not gated on `is_for_the_game`: a key pressed
        // during a session and released after it ended (or after focus
        // moved into a field) must still clear, or it stays held.
        if let Some(mask) = key_mask(&e.code()) {
            KEYBOARD.with(|k| k.set(k.get() & !mask));
        }
    });
    // Losing focus mid-press would otherwise leave the key held for as
    // long as the session runs.
    let blur = Closure::<dyn FnMut(web_sys::Event)>::new(|_| {
        KEYBOARD.with(|k| k.set(0));
        touch_clear();
    });

    let target: &web_sys::EventTarget = window.as_ref();
    let _ = target.add_event_listener_with_callback("keydown", down.as_ref().unchecked_ref());
    let _ = target.add_event_listener_with_callback("keyup", up.as_ref().unchecked_ref());
    let _ = target.add_event_listener_with_callback("blur", blur.as_ref().unchecked_ref());
    // Page-lifetime listeners: leaking the closures is the correct way
    // to say "never unregistered".
    down.forget();
    up.forget();
    blur.forget();
}

