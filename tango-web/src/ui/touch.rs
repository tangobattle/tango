//! On-screen touch controls, shown over the stage on coarse-pointer
//! screens (see the `.touch-controls` CSS; the session view is always
//! landscape there, rotated when the device is upright): a slide-aware
//! d-pad on the left, A/B on the right, L/R shoulders in the top
//! corners, and Start/Select at the bottom (the menu rides the shared
//! chip row — see `telemetry::CableOverlay`). Held buttons write mgba
//! joyflag bits straight into [`Runtime::touch_keys`]; the pump ORs
//! them with the mapped keyboard/gamepad state.

use dioxus::prelude::*;
use mgba::input::keys;

use super::use_ctx;

/// The d-pad face's CSS size. The direction math mirrors it; keep the
/// two in sync.
const DPAD_SIZE: f64 = 132.0;

/// Neutral-zone radius around the d-pad center, in px.
const DPAD_DEAD: f64 = 18.0;

const DPAD_MASK: u32 = keys::UP | keys::DOWN | keys::LEFT | keys::RIGHT;

#[component]
pub fn TouchControls() -> Element {
    let ctx = use_ctx();
    let runtime = ctx.runtime;
    // Mirror of the touch-held joyflags, for pressed styling.
    let held = use_signal(|| 0u32);
    // True while a finger owns the d-pad (pointer moves only steer
    // between directions mid-press).
    let dpad_active = use_signal(|| false);

    // Held bits must not outlive the overlay (rotation, session end).
    {
        let runtime = runtime.clone();
        use_drop(move || runtime.borrow_mut().touch_keys = 0);
    }

    let set = {
        let runtime = runtime.clone();
        move |mut held: Signal<u32>, mask: u32, bits: u32| {
            let next = (*held.peek() & !mask) | bits;
            if next != *held.peek() {
                held.set(next);
                runtime.borrow_mut().touch_keys = next;
            }
        }
    };

    // Element-relative touch position → direction bits (8-way; the
    // dead zone in the middle is neutral).
    let dpad_bits = |evt: &Event<PointerData>| -> u32 {
        let p = evt.data().element_coordinates();
        let dx = p.x - DPAD_SIZE / 2.0;
        let dy = p.y - DPAD_SIZE / 2.0;
        let mut bits = 0;
        if dx.hypot(dy) < DPAD_DEAD {
            return 0;
        }
        // A 22.5° bias toward the dominant axis keeps straight lines
        // easy and diagonals deliberate.
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
                class: if *held.read() & DPAD_MASK != 0 { "held" },
                onpointerdown: {
                    let set = set.clone();
                    let mut dpad_active = dpad_active;
                    move |evt: Event<PointerData>| {
                        dpad_active.set(true);
                        set(held, DPAD_MASK, dpad_bits(&evt));
                    }
                },
                onpointermove: {
                    let set = set.clone();
                    move |evt: Event<PointerData>| {
                        if *dpad_active.peek() {
                            set(held, DPAD_MASK, dpad_bits(&evt));
                        }
                    }
                },
                onpointerup: {
                    let set = set.clone();
                    let mut dpad_active = dpad_active;
                    move |_| {
                        dpad_active.set(false);
                        set(held, DPAD_MASK, 0);
                    }
                },
                onpointercancel: {
                    let set = set.clone();
                    let mut dpad_active = dpad_active;
                    move |_| {
                        dpad_active.set(false);
                        set(held, DPAD_MASK, 0);
                    }
                },
                onpointerleave: {
                    let set = set.clone();
                    let mut dpad_active = dpad_active;
                    move |_| {
                        dpad_active.set(false);
                        set(held, DPAD_MASK, 0);
                    }
                },
            }
            for (class , label , mask) in buttons {
                div {
                    class: "{class}",
                    class: if *held.read() & mask != 0 { "held" },
                    onpointerdown: {
                        let set = set.clone();
                        move |_| set(held, mask, mask)
                    },
                    onpointerup: {
                        let set = set.clone();
                        move |_| set(held, mask, 0)
                    },
                    onpointercancel: {
                        let set = set.clone();
                        move |_| set(held, mask, 0)
                    },
                    onpointerleave: {
                        let set = set.clone();
                        move |_| set(held, mask, 0)
                    },
                    "{label}"
                }
            }
        }
    }
}
