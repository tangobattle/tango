//! SDL3 gamepad input (same stack as the desktop client). Unlike the
//! browser backend, which rebuilds held state from a `getGamepads()`
//! snapshot every pump, SDL gives us an event stream: [`poll_into`]
//! drains the (singleton) event pump once per runtime pump and applies
//! button/axis deltas to the caller's `HeldState`. Device hotplug is
//! handled internally; a disconnect clears the gamepad half of the
//! held state so nothing reads as stuck-down.

use std::collections::HashMap;
use std::sync::Mutex;

use sdl3::event::Event as SdlEvent;
use sdl3::gamepad::Gamepad;
use sdl3::sys::joystick::SDL_JoystickID;
use sdl3::GamepadSubsystem;
use send_wrapper::SendWrapper;

use crate::platform::input::{GamepadAxis, GamepadButton, HeldState};
use crate::platform::sdl_init;

struct Context {
    gamepads: GamepadSubsystem,
    /// Keep `Gamepad` handles alive — `GamepadSubsystem::open` returns
    /// owned handles; if they drop, SDL stops emitting events for
    /// those devices.
    open: HashMap<u32, Gamepad>,
}

static GAMEPAD_CONTEXT: Mutex<Option<SendWrapper<Context>>> = Mutex::new(None);

/// Open every attached gamepad and stash the context in the global.
/// Call once at startup, after [`sdl_init::init`], on the main
/// thread. Failures are logged and turn subsequent [`poll_into`]
/// calls into no-ops — the app keeps running without gamepad support.
pub fn init() {
    let Some(sdl) = sdl_init::sdl() else {
        log::warn!("sdl3 gamepad init skipped: sdl not initialized");
        return;
    };
    match build_context(&sdl) {
        Ok(ctx) => *GAMEPAD_CONTEXT.lock().unwrap() = Some(SendWrapper::new(ctx)),
        Err(e) => log::warn!("sdl3 gamepad init failed: {e}"),
    }
}

fn build_context(sdl: &sdl3::Sdl) -> Result<Context, String> {
    let gamepads = sdl.gamepad().map_err(|e| e.to_string())?;
    let mut ctx = Context {
        gamepads,
        open: HashMap::new(),
    };
    // Open every gamepad already attached at startup. Hotplug is
    // handled in `poll_into` via `ControllerDeviceAdded`.
    if let Ok(ids) = ctx.gamepads.gamepads() {
        for id in ids {
            match ctx.gamepads.open(id) {
                Ok(g) => {
                    ctx.open.insert(id.0, g);
                }
                Err(e) => log::warn!("failed to open gamepad {}: {e}", id.0),
            }
        }
    }
    Ok(ctx)
}

/// Drain SDL's queued events and apply the gamepad-relevant ones to
/// `held`. Keyboard state in `held` is untouched (that comes from the
/// UI's key events). No-op if SDL never initialized.
pub fn poll_into(held: &mut HeldState) {
    let Some(mut pump) = sdl_init::event_pump() else { return };
    let mut guard = GAMEPAD_CONTEXT.lock().unwrap();
    let Some(ctx) = guard.as_mut() else { return };
    while let Some(event) = pump.poll_event() {
        match event {
            SdlEvent::ControllerButtonDown { button, .. } => {
                held.set_button(GamepadButton::from_sdl3(button), true);
            }
            SdlEvent::ControllerButtonUp { button, .. } => {
                held.set_button(GamepadButton::from_sdl3(button), false);
            }
            SdlEvent::ControllerAxisMotion { axis, value, .. } => {
                use sdl3::gamepad::Axis as A;
                let axis = match axis {
                    A::LeftX => GamepadAxis::LeftStickX,
                    A::LeftY => GamepadAxis::LeftStickY,
                    A::RightX => GamepadAxis::RightStickX,
                    A::RightY => GamepadAxis::RightStickY,
                    A::TriggerLeft => GamepadAxis::TriggerLeft,
                    A::TriggerRight => GamepadAxis::TriggerRight,
                };
                held.set_axis(axis, (value as f32 / 0x7FFF as f32).clamp(-1.0, 1.0));
            }
            SdlEvent::ControllerDeviceAdded { which, .. } => {
                let id = SDL_JoystickID(which);
                match ctx.gamepads.open(id) {
                    Ok(g) => {
                        ctx.open.insert(which, g);
                    }
                    Err(e) => log::warn!("failed to open hotplug gamepad {which}: {e}"),
                }
            }
            SdlEvent::ControllerDeviceRemoved { which, .. } => {
                ctx.open.remove(&which);
                // A disconnected pad must not read as still-held; any
                // remaining pads re-assert on their next events.
                held.clear_gamepad();
            }
            _ => {}
        }
    }
}
