//! Browser gamepad input: a per-pump `navigator.getGamepads()`
//! snapshot folded into [`HeldState`](super::input::HeldState). No
//! event stream — the held gamepad state is simply rebuilt every pump,
//! which also makes disconnects self-healing.

use wasm_bindgen::JsCast;

use crate::platform::input::{GamepadAxis, GamepadButton, HeldState};

/// W3C "standard" gamepad mapping, button index → binding button.
/// Indices 6/7 (triggers) are analog and surface as
/// [`GamepadAxis::TriggerLeft`]/[`TriggerRight`] instead.
const STANDARD_BUTTONS: [Option<GamepadButton>; 18] = [
    Some(GamepadButton::South),         // 0
    Some(GamepadButton::East),          // 1
    Some(GamepadButton::West),          // 2
    Some(GamepadButton::North),         // 3
    Some(GamepadButton::LeftShoulder),  // 4
    Some(GamepadButton::RightShoulder), // 5
    None,                               // 6: left trigger (axis)
    None,                               // 7: right trigger (axis)
    Some(GamepadButton::Select),        // 8
    Some(GamepadButton::Start),         // 9
    Some(GamepadButton::LeftThumb),     // 10
    Some(GamepadButton::RightThumb),    // 11
    Some(GamepadButton::DPadUp),        // 12
    Some(GamepadButton::DPadDown),      // 13
    Some(GamepadButton::DPadLeft),      // 14
    Some(GamepadButton::DPadRight),     // 15
    Some(GamepadButton::Mode),          // 16
    Some(GamepadButton::Touchpad),      // 17
];

/// Rebuild the held gamepad state from a fresh snapshot of every
/// connected pad. Keyboard state in `held` is untouched.
pub fn poll_into(held: &mut HeldState) {
    held.clear_gamepad();
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(pads) = window.navigator().get_gamepads() else {
        return;
    };
    for pad in pads.iter() {
        let Ok(pad) = pad.dyn_into::<web_sys::Gamepad>() else {
            continue;
        };
        if !pad.connected() {
            continue;
        }
        // Non-"standard" mappings still get index-order button folding —
        // usable, if not pretty; the binding UI shows what fires.
        let buttons = pad.buttons();
        for (i, b) in buttons.iter().enumerate() {
            let Ok(b) = b.dyn_into::<web_sys::GamepadButton>() else {
                continue;
            };
            match STANDARD_BUTTONS.get(i).copied().flatten() {
                Some(button) => {
                    if b.pressed() {
                        held.set_button(button, true);
                    }
                }
                None if i == 6 => held.set_axis(GamepadAxis::TriggerLeft, b.value() as f32),
                None if i == 7 => held.set_axis(GamepadAxis::TriggerRight, b.value() as f32),
                None => {}
            }
        }
        let axes = pad.axes();
        let axis = |i: u32| axes.get(i).as_f64().unwrap_or(0.0) as f32;
        held.set_axis(GamepadAxis::LeftStickX, axis(0));
        held.set_axis(GamepadAxis::LeftStickY, axis(1));
        held.set_axis(GamepadAxis::RightStickX, axis(2));
        held.set_axis(GamepadAxis::RightStickY, axis(3));
    }
}
