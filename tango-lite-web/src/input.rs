//! One GBA joyflag word, from touch, a keyboard, and a gamepad.
//!
//! The session takes the whole input at once ([`Session::set_input`]),
//! so every source folds into one held-buttons word that the pump reads
//! each frame. Three rather than one because the same page is a phone, a
//! laptop, and a phone with a controller clipped to it; they simply OR
//! together, and none of them has to know about the others.
//!
//! [`Session::set_input`]: tango_session::Session::set_input

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use tango_session::keys;

thread_local! {
    static TOUCH: Cell<u32> = const { Cell::new(0) };
    static KEYBOARD: Cell<u32> = const { Cell::new(0) };
    /// Per-pad, because a stick and a d-pad drive the same four bits:
    /// merged into one word, centring the stick would release a d-pad
    /// direction that is still held. Keyed by device so unplugging one
    /// can't leave another's buttons stuck down.
    static PADS: RefCell<HashMap<gamepad_facade::Id, Pad>> = RefCell::new(HashMap::new());
    /// The stylus, for a console with a touch screen: where it is
    /// pressed on that screen, in the screen's own pixels, or `None`
    /// lifted. Written by the play screen's pointer handlers, read by
    /// the pump alongside [`joyflags`].
    static STYLUS: Cell<Option<(u16, u16)>> = const { Cell::new(None) };
}

/// One controller's contribution, split by what produced it.
#[derive(Default, Clone, Copy)]
struct Pad {
    buttons: u32,
    axes: u32,
}

/// Everything currently held, from any source.
pub fn joyflags() -> u32 {
    let pads = PADS.with(|pads| {
        pads.borrow()
            .values()
            .fold(0, |held, pad| held | pad.buttons | pad.axes)
    });
    TOUCH.with(|t| t.get()) | KEYBOARD.with(|k| k.get()) | pads
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
    STYLUS.with(|s| s.set(None));
}

/// Where the stylus is pressed, or `None` lifted.
pub fn stylus() -> Option<(u16, u16)> {
    STYLUS.with(|s| s.get())
}

/// Press, drag, or lift the stylus.
pub fn stylus_set(at: Option<(u16, u16)>) {
    STYLUS.with(|s| s.set(at));
}

/// Every direction bit, as one field for [`touch_set`].
pub const DPAD: u32 = keys::UP | keys::DOWN | keys::LEFT | keys::RIGHT;

/// How far a stick has to leave centre to count as a direction. The
/// games are digital, so this is a threshold rather than a curve; half
/// travel is far enough to be deliberate and near enough that diagonals
/// still come out.
const STICK_THRESHOLD: f32 = 0.5;

/// Start listening for controllers. Once, at startup — the browser's
/// Gamepad API only reveals a pad after the user presses something on
/// it, so there is nothing to wait for and nothing to re-init.
pub fn install_gamepads() {
    gamepad_facade::init("Tango Lite");
}

/// Drain whatever the controllers have done since the last call.
///
/// Driven from the pump rather than an event listener because the
/// browser's Gamepad API has no events to listen to — it is a snapshot
/// you diff, which is exactly what the facade does inside `next_event`.
pub fn poll_gamepads() {
    while let Some(event) = gamepad_facade::next_event() {
        PADS.with(|pads| {
            let mut pads = pads.borrow_mut();
            match event.kind {
                gamepad_facade::EventKind::Connected => {
                    pads.entry(event.id).or_default();
                }
                // Drop its state rather than leaving the bits it was
                // holding folded into every later frame.
                gamepad_facade::EventKind::Disconnected => {
                    pads.remove(&event.id);
                }
                gamepad_facade::EventKind::ButtonDown(button) => {
                    if let Some(mask) = button_mask(button) {
                        pads.entry(event.id).or_default().buttons |= mask;
                    }
                }
                gamepad_facade::EventKind::ButtonUp(button) => {
                    if let Some(mask) = button_mask(button) {
                        pads.entry(event.id).or_default().buttons &= !mask;
                    }
                }
                gamepad_facade::EventKind::AxisMotion { axis, value } => {
                    let (negative, positive) = match axis {
                        gamepad_facade::Axis::LeftX => (keys::LEFT, keys::RIGHT),
                        // Up is negative, in SDL's convention and the
                        // browser's alike.
                        gamepad_facade::Axis::LeftY => (keys::UP, keys::DOWN),
                        // The right stick and the triggers have nothing
                        // to drive on a GBA.
                        _ => return,
                    };
                    let held = &mut pads.entry(event.id).or_default().axes;
                    *held &= !(negative | positive);
                    if value <= -STICK_THRESHOLD {
                        *held |= negative;
                    } else if value >= STICK_THRESHOLD {
                        *held |= positive;
                    }
                }
            }
        });
    }
}

/// The desktop app's default pad layout, which is the legacy app's:
/// face buttons in the Xbox arrangement, shoulders for L/R.
fn button_mask(button: gamepad_facade::Button) -> Option<u32> {
    use gamepad_facade::Button as B;
    Some(match button {
        B::South => keys::A,
        B::East => keys::B,
        B::LeftShoulder => keys::L,
        B::RightShoulder => keys::R,
        B::Start => keys::START,
        B::Back => keys::SELECT,
        B::DPadUp => keys::UP,
        B::DPadDown => keys::DOWN,
        B::DPadLeft => keys::LEFT,
        B::DPadRight => keys::RIGHT,
        _ => return None,
    })
}

/// Forget every controller's held state. Used alongside
/// [`touch_clear`] when a session goes away mid-press.
pub fn gamepads_clear() {
    PADS.with(|pads| pads.borrow_mut().clear());
}

/// The desktop app's default bindings, key for key: arrows to move,
/// Z/X for A/B, A/S for L/R, Enter/Space for Start/Select. Deliberately
/// not configurable: a lite build that needs a remapping screen isn't
/// lite. Q/W carry the DS's extra X/Y pair — dead bits on a GBA
/// session, which masks its input word anyway.
fn key_mask(code: &str) -> Option<u32> {
    Some(match code {
        "ArrowUp" => keys::UP,
        "ArrowDown" => keys::DOWN,
        "ArrowLeft" => keys::LEFT,
        "ArrowRight" => keys::RIGHT,
        "KeyZ" => keys::A,
        "KeyX" => keys::B,
        "KeyQ" => keys::X,
        "KeyW" => keys::Y,
        "KeyA" => keys::L,
        "KeyS" => keys::R,
        "Enter" => keys::START,
        "Space" => keys::SELECT,
        _ => return None,
    })
}

/// Whether this key event belongs to the emulator rather than to the
/// page.
///
/// Three ways it doesn't, and all three matter: half the button map is
/// ordinary letters (`zxas`) plus Space, so a listener that takes them
/// unconditionally makes every text field on the site unusable — you
/// cannot type a link code or a nickname. Which is exactly what
/// happened.
fn is_for_the_game(event: &web_sys::KeyboardEvent) -> bool {
    // Nothing to play.
    if !crate::engine::is_running() {
        return false;
    }
    // A shortcut, not a button. (Shift isn't tested: alone it's no
    // browser shortcut, and holding it shouldn't swallow a button.)
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
        // A repeat isn't a new press, and the arrows and Space scroll
        // the page if we let them through.
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
