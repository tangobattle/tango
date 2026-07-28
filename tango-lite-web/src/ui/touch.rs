//! The on-screen pad: an overlay across the stage on coarse-pointer
//! screens — a slide-aware D-pad bottom left, A/B bottom right, L/R in
//! the top corners, Start/Select as pills along the bottom.
//!
//! The D-pad is one control, not four buttons. Where the finger lands
//! inside its circle is the direction, a dead zone in the middle is
//! neutral, and `pointermove` re-steers mid-press — so going from left
//! to up-left to up is one continuous slide, which is how a thumb
//! actually moves and what a grid of nine buttons can't do. The 22.5°
//! bias toward the dominant axis keeps straight lines easy to hold and
//! makes diagonals deliberate.
//!
//! The overlay itself is inert (`pointer-events: none`); only the
//! controls take touches, so the stage between them stays reachable.
//! Held bits are cleared on drop, or a session torn down mid-press
//! leaves a direction stuck on.

use dioxus::prelude::*;

use tango_session::keys;

use crate::input;

/// The D-pad's CSS size. The direction maths below mirrors it — keep
/// the two in sync.
const DPAD_SIZE: f64 = 132.0;

/// Neutral radius around the centre, in px.
const DPAD_DEAD: f64 = 18.0;

#[component]
pub fn TouchControls() -> Element {
    // Mirrors the touch word purely so held buttons can be styled; the
    // engine reads `input::joyflags` directly.
    let mut held = use_signal(|| 0u32);
    // Whether a finger owns the D-pad. `pointermove` only steers while
    // one does — otherwise a finger crossing the pad on its way
    // somewhere else would grab it.
    let mut dpad_active = use_signal(|| false);

    use_drop(move || input::touch_clear());

    let mut set = move |mask: u32, bits: u32| {
        let next = (*held.peek() & !mask) | bits;
        if next != *held.peek() {
            held.set(next);
            input::touch_set(mask, bits);
        }
    };

    // Element-relative touch position → direction bits.
    let direction = |event: &Event<PointerData>| -> u32 {
        let point = event.data().element_coordinates();
        let (dx, dy) = (point.x - DPAD_SIZE / 2.0, point.y - DPAD_SIZE / 2.0);
        if dx.hypot(dy) < DPAD_DEAD {
            return 0;
        }
        let mut bits = 0;
        if dx.abs() > dy.abs() * 0.414 {
            bits |= if dx > 0.0 { keys::RIGHT } else { keys::LEFT };
        }
        if dy.abs() > dx.abs() * 0.414 {
            bits |= if dy > 0.0 { keys::DOWN } else { keys::UP };
        }
        bits
    };

    let buttons: [(&str, &str, u32); 6] = [
        ("tc-btn tc-a", "A", keys::A),
        ("tc-btn tc-b", "B", keys::B),
        ("tc-btn tc-l", "L", keys::L),
        ("tc-btn tc-r", "R", keys::R),
        ("tc-btn tc-pill tc-start", "start", keys::START),
        ("tc-btn tc-pill tc-select", "select", keys::SELECT),
    ];

    rsx! {
        div { class: "touch-controls",
            div {
                class: "tc-dpad",
                class: if *held.read() & input::DPAD != 0 { "held" },
                onpointerdown: move |event: Event<PointerData>| {
                    dpad_active.set(true);
                    set(input::DPAD, direction(&event));
                },
                onpointermove: move |event: Event<PointerData>| {
                    if *dpad_active.peek() {
                        set(input::DPAD, direction(&event));
                    }
                },
                onpointerup: move |_| {
                    dpad_active.set(false);
                    set(input::DPAD, 0);
                },
                onpointercancel: move |_| {
                    dpad_active.set(false);
                    set(input::DPAD, 0);
                },
                // No pointer capture, so a thumb that slides off the pad
                // releases it instead of steering from outside.
                onpointerleave: move |_| {
                    dpad_active.set(false);
                    set(input::DPAD, 0);
                },
            }
            for (class , label , mask) in buttons {
                div {
                    key: "{label}",
                    class: "{class}",
                    class: if *held.read() & mask != 0 { "held" },
                    onpointerdown: move |_| set(mask, mask),
                    onpointerup: move |_| set(mask, 0),
                    onpointercancel: move |_| set(mask, 0),
                    onpointerleave: move |_| set(mask, 0),
                    "{label}"
                }
            }
        }
    }
}
