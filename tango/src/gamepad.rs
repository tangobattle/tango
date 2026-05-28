//! Global SDL3 gamepad helper. Replaces the previous `gilrs`
//! dependency.
//!
//! SDL3 itself is initialized in [`crate::sdl_init`] — this
//! module just borrows the global `Sdl` to spin up an
//! `EventPump` and `GamepadSubsystem` on the main thread. Both
//! are `!Send` and the crate enforces "first thread to call
//! `SDL_Init` is the only one that can pump", so we wrap the
//! local context in a `Send` newtype and hand-check the owning
//! thread at every access.
//!
//! The event pump is a singleton — only one `EventPump` can exist
//! at a time per the sdl3 crate's reference counting — so a
//! single shared pumper is the only viable shape. [`pump`] drains
//! the entire pump once and emits gamepad-relevant events via a
//! callback. Auto-opens gamepads on `ControllerDeviceAdded` and
//! closes on `Removed` so the caller doesn't have to.
//!
//! Convenience: [`GamepadEvent`] is the small subset we actually
//! care about (button up/down, axis motion, device removed).
//! Keeps the call sites independent of `sdl3`'s richer event
//! enum.

use std::collections::HashMap;
use std::thread::ThreadId;

use parking_lot::Mutex;
use sdl3::event::Event as SdlEvent;
use sdl3::gamepad::{Button, Gamepad};
use sdl3::sys::joystick::SDL_JoystickID;
use sdl3::{EventPump, GamepadSubsystem};

use crate::input::GamepadAxis;
use crate::sdl_init;

/// What `input_capture` / settings binding-capture care about. Keeps
/// the surface narrow so we don't propagate `sdl3` types into the
/// rest of the UI. Axis events are pre-normalized: SDL3's raw i16
/// `[-32768, 32767]` reading is mapped to `f32` in `[-1, 1]`, with Y
/// flipped so positive means "stick pushed up" (matches the default
/// binding map; SDL3's raw joystick convention is the opposite).
pub enum GamepadEvent {
    ButtonDown(Button),
    ButtonUp(Button),
    AxisMotion { axis: GamepadAxis, value: f32 },
    DeviceRemoved,
}

struct Context {
    pump: EventPump,
    gamepads: GamepadSubsystem,
    /// Keep `Gamepad` handles alive — `GamepadSubsystem::open` returns
    /// owned handles; if they drop, SDL stops emitting events for
    /// those devices.
    open: HashMap<u32, Gamepad>,
}

struct SendContext {
    inner: Context,
    owner: ThreadId,
}

/// SAFETY: `get_mut` panics if accessed from any thread other than
/// the one that constructed the [`SendContext`], so the `!Send`
/// fields inside `Context` are only ever touched on their owning
/// thread despite the wrapper itself being `Send`-able into the
/// global `Mutex`.
unsafe impl Send for SendContext {}

impl SendContext {
    fn new(inner: Context) -> Self {
        Self {
            inner,
            owner: std::thread::current().id(),
        }
    }

    fn get_mut(&mut self) -> &mut Context {
        let cur = std::thread::current().id();
        assert_eq!(
            cur, self.owner,
            "gamepad context accessed from thread {cur:?} but was initialized on {:?}",
            self.owner,
        );
        &mut self.inner
    }
}

static GAMEPAD_CONTEXT: Mutex<Option<SendContext>> = Mutex::new(None);

/// Open every attached gamepad and stash the context in the
/// global. Call this once at startup, after
/// [`crate::sdl_init::init`], on the iced/winit main thread.
/// Failures are logged and turn subsequent [`pump`] calls into
/// no-ops — the app keeps running without gamepad support.
pub fn init() {
    match sdl_init::with_sdl(build_context) {
        Some(Ok(ctx)) => *GAMEPAD_CONTEXT.lock() = Some(SendContext::new(ctx)),
        Some(Err(e)) => log::warn!("sdl3 gamepad init failed: {e}"),
        None => log::warn!("sdl3 gamepad init skipped: sdl not initialized"),
    }
}

fn build_context(sdl: &sdl3::Sdl) -> Result<Context, String> {
    let gamepads = sdl.gamepad().map_err(|e| e.to_string())?;
    let pump = sdl.event_pump().map_err(|e| e.to_string())?;
    let mut ctx = Context {
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
/// internally — callers only see the narrow [`GamepadEvent`]. No-op
/// if [`init`] never succeeded.
pub fn pump(mut on_event: impl FnMut(GamepadEvent)) {
    let mut guard = GAMEPAD_CONTEXT.lock();
    let Some(wrapper) = guard.as_mut() else { return };
    let ctx = wrapper.get_mut();
    while let Some(event) = ctx.pump.poll_event() {
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
                    _ => continue,
                };
                let mut v = (value as f32 / 32767.0).clamp(-1.0, 1.0);
                if matches!(axis, GamepadAxis::LeftStickY | GamepadAxis::RightStickY) {
                    v = -v;
                }
                on_event(GamepadEvent::AxisMotion { axis, value: v });
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
