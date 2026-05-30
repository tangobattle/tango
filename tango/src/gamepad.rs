//! Global SDL3 gamepad helper. Replaces the previous `gilrs`
//! dependency.
//!
//! SDL3 itself is initialized in [`crate::sdl_init`] — this module
//! borrows the global `Sdl` to spin up a `GamepadSubsystem` on the
//! main thread, and borrows the global `EventPump` (owned by
//! `sdl_init`) to drain input. Both are `!Send`, so the local
//! context lives in a [`send_wrapper::SendWrapper`], which is
//! `Send`/`Sync` but panics if touched off its owning thread.
//!
//! [`pump`] drains the entire event pump once and emits
//! gamepad-relevant events via a callback. Auto-opens gamepads on
//! `ControllerDeviceAdded` and closes on `Removed` so the caller
//! doesn't have to.
//!
//! Convenience: [`GamepadEvent`] is the small subset we actually
//! care about (button up/down, axis motion, device removed).
//! Keeps the call sites independent of `sdl3`'s richer event
//! enum.

use std::collections::HashMap;
use std::sync::Mutex;

use sdl3::event::Event as SdlEvent;
use sdl3::gamepad::{Button, Gamepad};
use sdl3::sys::joystick::SDL_JoystickID;
use sdl3::GamepadSubsystem;
use send_wrapper::SendWrapper;

use crate::input::GamepadAxis;
use crate::sdl_init;

/// What `input_capture` / settings binding-capture care about. Keeps
/// the surface narrow so we don't propagate `sdl3` types into the
/// rest of the UI. Axis events are pre-normalized: SDL3's raw i16
/// `[-32768, 32767]` reading is mapped to `f32` in `[-1, 1]`, in
/// SDL3's own convention (stick-up is negative Y). The default
/// binding map accounts for that via each axis binding's `AxisDir`,
/// so the sign isn't massaged here.
pub enum GamepadEvent {
    ButtonDown(Button),
    ButtonUp(Button),
    AxisMotion { axis: GamepadAxis, value: f32 },
    DeviceRemoved,
}

struct Context {
    gamepads: GamepadSubsystem,
    /// Keep `Gamepad` handles alive — `GamepadSubsystem::open` returns
    /// owned handles; if they drop, SDL stops emitting events for
    /// those devices.
    open: HashMap<u32, Gamepad>,
}

static GAMEPAD_CONTEXT: Mutex<Option<SendWrapper<Context>>> = Mutex::new(None);

/// Open every attached gamepad and stash the context in the global.
/// Call this once at startup, after [`crate::sdl_init::init`], on the
/// iced/winit main thread. Failures are logged and turn subsequent
/// [`pump`] calls into no-ops — the app keeps running without gamepad
/// support.
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
    // handled in `pump` via `ControllerDeviceAdded`.
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

/// Drain every event currently queued in SDL and emit the
/// gamepad-relevant ones via `on_event`. Handles device add/remove
/// internally — callers only see the narrow [`GamepadEvent`]. No-op
/// if [`crate::sdl_init::init`] / [`init`] never succeeded.
pub fn pump(mut on_event: impl FnMut(GamepadEvent)) {
    // Event pump lives in `sdl_init` (it's an SDL singleton); borrow
    // it for the drain, plus our own gamepad context for hotplug.
    let Some(mut pump) = sdl_init::event_pump() else { return };
    let mut guard = GAMEPAD_CONTEXT.lock().unwrap();
    let Some(ctx) = guard.as_mut() else { return };
    while let Some(event) = pump.poll_event() {
        match event {
            SdlEvent::ControllerButtonDown { button, .. } => {
                on_event(GamepadEvent::ButtonDown(button));
            }
            SdlEvent::ControllerButtonUp { button, .. } => {
                on_event(GamepadEvent::ButtonUp(button));
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
                on_event(GamepadEvent::AxisMotion {
                    axis,
                    value: (value as f32 / 0x7FFF as f32).clamp(-1.0, 1.0),
                });
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
                on_event(GamepadEvent::DeviceRemoved);
            }
            _ => {}
        }
    }
}
