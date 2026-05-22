//! Thread-local SDL3 gamepad helper. Replaces the previous `gilrs`
//! dependency.
//!
//! SDL3's `Sdl`, `EventPump`, and `GamepadSubsystem` are all `!Send`
//! and the crate enforces "first thread that calls `init()` is the
//! only one that can pump", so the context is held in a `thread_local!`
//! and lazy-initialized on first use. All callers must run on the
//! iced/winit main thread (widget `update()` and `view()` always do).
//!
//! The event pump is a singleton — only one `EventPump` can exist at a
//! time per the sdl3 crate's reference counting — so a single shared
//! pumper is the only viable shape. [`pump`] drains the entire pump
//! once and emits gamepad-relevant events via a callback. Auto-opens
//! gamepads on `ControllerDeviceAdded` and closes on `Removed` so the
//! caller doesn't have to.
//!
//! Convenience: [`GamepadEvent`] is the small subset we actually care
//! about (button up/down, axis motion, device removed). Keeps the call
//! sites independent of `sdl3`'s richer event enum.

use std::cell::RefCell;
use std::collections::HashMap;

use sdl3::event::Event as SdlEvent;
use sdl3::gamepad::{Axis, Button, Gamepad};
use sdl3::sys::joystick::SDL_JoystickID;
use sdl3::{EventPump, GamepadSubsystem, Sdl};

/// What `input_capture` / settings binding-capture care about. Keeps
/// the surface narrow so we don't propagate `sdl3` types into the
/// rest of the UI.
pub enum GamepadEvent {
    ButtonDown(Button),
    ButtonUp(Button),
    AxisMotion { axis: Axis, value: i16 },
    DeviceRemoved,
}

struct Context {
    _sdl: Sdl,
    pump: EventPump,
    gamepads: GamepadSubsystem,
    /// Keep `Gamepad` handles alive — `GamepadSubsystem::open` returns
    /// owned handles; if they drop, SDL stops emitting events for
    /// those devices.
    open: HashMap<u32, Gamepad>,
}

thread_local! {
    static SDL_CONTEXT: RefCell<Option<Context>> = const { RefCell::new(None) };
    static INIT_FAILED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn ensure_context<R>(f: impl FnOnce(&mut Context) -> R) -> Option<R> {
    if INIT_FAILED.with(|c| c.get()) {
        return None;
    }
    SDL_CONTEXT.with(|cell| {
        let mut cell = cell.borrow_mut();
        if cell.is_none() {
            // Per the SDL3 gamepad example: needed on Windows so the
            // joystick subsystem spins up its own polling thread when
            // we don't have a video subsystem hooked into the message
            // loop.
            sdl3::hint::set("SDL_JOYSTICK_THREAD", "1");
            match init() {
                Ok(ctx) => *cell = Some(ctx),
                Err(e) => {
                    log::warn!("sdl3 gamepad init failed: {e}");
                    INIT_FAILED.with(|c| c.set(true));
                    return None;
                }
            }
        }
        Some(f(cell.as_mut().unwrap()))
    })
}

fn init() -> Result<Context, String> {
    let sdl = sdl3::init().map_err(|e| e.to_string())?;
    let gamepads = sdl.gamepad().map_err(|e| e.to_string())?;
    let pump = sdl.event_pump().map_err(|e| e.to_string())?;
    let mut ctx = Context {
        _sdl: sdl,
        pump,
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
/// internally — callers only see the narrow [`GamepadEvent`].
pub fn pump(mut on_event: impl FnMut(GamepadEvent)) {
    ensure_context(|ctx| {
        while let Some(event) = ctx.pump.poll_event() {
            match event {
                SdlEvent::ControllerButtonDown { button, .. } => {
                    on_event(GamepadEvent::ButtonDown(button));
                }
                SdlEvent::ControllerButtonUp { button, .. } => {
                    on_event(GamepadEvent::ButtonUp(button));
                }
                SdlEvent::ControllerAxisMotion { axis, value, .. } => {
                    on_event(GamepadEvent::AxisMotion { axis, value });
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
    });
}
