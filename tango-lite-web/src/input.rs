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

/// Install the page-wide key listeners. Once, at startup: they cost
/// nothing while no session is running, and installing them per screen
/// would lose a key held across a screen change.
pub fn install_keyboard() {
    let Some(window) = web_sys::window() else { return };

    let down = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(|e: web_sys::KeyboardEvent| {
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
        if let Some(mask) = key_mask(&e.code()) {
            e.prevent_default();
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

