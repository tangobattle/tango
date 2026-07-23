//! Minimal SDL3 gamepad support, decoupled from the rest of SDL3.
//!
//! The vendored SDL3 build this crate links is trimmed to the joystick
//! subsystem (which the gamepad API sits on) plus HIDAPI — no audio,
//! video, render, or GPU. See `Cargo.toml` for the exact feature trim.
//! The API here never surfaces an `sdl3` type, so callers depend on
//! this crate instead of `sdl3`: they get [`Button`], [`Axis`], and
//! [`GamepadEvent`], drive input with [`init`] + [`pump`], and stay
//! oblivious to SDL.
//!
//! # Threading
//!
//! SDL3's `Sdl` handle (and the `EventPump` derived from it) are
//! `!Send`, and the library enforces that init happens on the main
//! thread (via a thread-local check inside `Sdl::new`). So [`init`]
//! runs once from the app's main thread and stashes every handle in
//! [`send_wrapper::SendWrapper`] globals: `SendWrapper` is `Send`/`Sync`
//! but panics if the inner value is touched from any thread other than
//! the one that built it, which is exactly the guarantee we need.
//! [`pump`] must likewise be called on that same thread.
//!
//! The `EventPump` is an SDL singleton (the `sdl3` crate ref-counts it,
//! so only one can exist at a time), which is why it lives here in a
//! global rather than being handed back to the caller.

use std::collections::HashMap;
use std::sync::Mutex;

use sdl3::event::Event as SdlEvent;
use sdl3::gamepad::{Button as SdlButton, Gamepad};
use sdl3::sys::joystick::SDL_JoystickID;
use sdl3::{EventPump, GamepadSubsystem, Sdl};
use send_wrapper::SendWrapper;

/// A gamepad button, mirroring SDL3's standard layout 1:1. Beyond the
/// usual Xbox/PS face/shoulder/d-pad set this covers the extras on
/// fancier pads: the `Misc*` share/capture-style buttons, the four back
/// paddles, and the touchpad click. Triggers are **not** buttons here —
/// SDL reports them as axes, so they come through [`Axis::TriggerLeft`]
/// / [`Axis::TriggerRight`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Button {
    South, // A on Xbox, X on PS
    East,  // B on Xbox, Circle on PS
    West,  // X on Xbox, Square on PS
    North, // Y on Xbox, Triangle on PS
    Back,  // Select / Share
    Start,
    Guide, // Guide / PS button
    LeftStick,
    RightStick,
    LeftShoulder,
    RightShoulder,
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
    Misc1,
    Misc2,
    Misc3,
    Misc4,
    Misc5,
    Misc6,
    RightPaddle1,
    LeftPaddle1,
    RightPaddle2,
    LeftPaddle2,
    Touchpad,
}

impl Button {
    fn from_sdl(b: SdlButton) -> Self {
        match b {
            SdlButton::South => Self::South,
            SdlButton::East => Self::East,
            SdlButton::West => Self::West,
            SdlButton::North => Self::North,
            SdlButton::Back => Self::Back,
            SdlButton::Start => Self::Start,
            SdlButton::Guide => Self::Guide,
            SdlButton::LeftStick => Self::LeftStick,
            SdlButton::RightStick => Self::RightStick,
            SdlButton::LeftShoulder => Self::LeftShoulder,
            SdlButton::RightShoulder => Self::RightShoulder,
            SdlButton::DPadUp => Self::DPadUp,
            SdlButton::DPadDown => Self::DPadDown,
            SdlButton::DPadLeft => Self::DPadLeft,
            SdlButton::DPadRight => Self::DPadRight,
            SdlButton::Misc1 => Self::Misc1,
            SdlButton::Misc2 => Self::Misc2,
            SdlButton::Misc3 => Self::Misc3,
            SdlButton::Misc4 => Self::Misc4,
            SdlButton::Misc5 => Self::Misc5,
            SdlButton::Misc6 => Self::Misc6,
            SdlButton::RightPaddle1 => Self::RightPaddle1,
            SdlButton::LeftPaddle1 => Self::LeftPaddle1,
            SdlButton::RightPaddle2 => Self::RightPaddle2,
            SdlButton::LeftPaddle2 => Self::LeftPaddle2,
            SdlButton::Touchpad => Self::Touchpad,
        }
    }
}

/// A gamepad analog axis, mirroring SDL3's naming. Values delivered by
/// [`pump`] are pre-normalized to `f32` in `[-1, 1]` in SDL's own
/// convention (stick-up is negative Y).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Axis {
    LeftX,
    LeftY,
    RightX,
    RightY,
    TriggerLeft,
    TriggerRight,
}

/// The narrow slice of gamepad input this crate emits. Keeping the
/// surface this small is what lets callers stay independent of `sdl3`'s
/// much richer event enum.
#[derive(Clone, Copy, Debug)]
pub enum GamepadEvent {
    ButtonDown(Button),
    ButtonUp(Button),
    AxisMotion { axis: Axis, value: f32 },
    /// A controller was unplugged. Callers should clear any held
    /// gamepad state so disconnected buttons don't read as still-down.
    DeviceRemoved,
}

/// The canonical SDL owner. Held for its lifetime so SDL stays inited;
/// dropping it (at process exit) runs `SDL_Quit`.
static SDL: Mutex<Option<SendWrapper<Sdl>>> = Mutex::new(None);
/// The singleton event pump, drained by [`pump`].
static EVENT_PUMP: Mutex<Option<SendWrapper<EventPump>>> = Mutex::new(None);
/// The gamepad subsystem plus the currently-open device handles.
static GAMEPAD_CONTEXT: Mutex<Option<SendWrapper<Context>>> = Mutex::new(None);

struct Context {
    gamepads: GamepadSubsystem,
    /// Keep `Gamepad` handles alive — `GamepadSubsystem::open` returns
    /// owned handles; if they drop, SDL stops emitting events for those
    /// devices.
    open: HashMap<u32, Gamepad>,
}

/// Initialize SDL3 and warm the gamepad context. Call once at startup,
/// on the app's main thread (the same thread that will later call
/// [`pump`]). Opening every attached controller up front means the
/// first [`pump`] doesn't pay for `SDL_Init` + device enumeration.
///
/// `app_name` is handed to SDL via the `SDL_APP_NAME` hint (used for
/// things like D-Bus identity). Any failure is logged and turns
/// subsequent [`pump`] calls into no-ops — the app keeps running
/// without gamepad support rather than taking the process down.
pub fn init(app_name: &str) {
    // Per the SDL3 gamepad example: needed on Windows so the joystick
    // subsystem spins up its own polling thread when there's no video
    // subsystem hooked into the message loop (there never is here).
    sdl3::hint::set("SDL_JOYSTICK_THREAD", "1");
    sdl3::hint::set("SDL_APP_NAME", app_name);

    let sdl = match sdl3::init() {
        Ok(s) => s,
        Err(e) => {
            log::warn!("sdl3 init failed: {e}");
            return;
        }
    };

    // Grab the (singleton) event pump before moving `sdl` into the
    // global.
    match sdl.event_pump() {
        Ok(pump) => *EVENT_PUMP.lock().unwrap() = Some(SendWrapper::new(pump)),
        Err(e) => log::warn!("sdl3 event pump init failed: {e}"),
    }

    match build_context(&sdl) {
        Ok(ctx) => *GAMEPAD_CONTEXT.lock().unwrap() = Some(SendWrapper::new(ctx)),
        Err(e) => log::warn!("sdl3 gamepad init failed: {e}"),
    }

    *SDL.lock().unwrap() = Some(SendWrapper::new(sdl));
}

fn build_context(sdl: &Sdl) -> Result<Context, String> {
    let gamepads = sdl.gamepad().map_err(|e| e.to_string())?;
    let mut ctx = Context {
        gamepads,
        open: HashMap::new(),
    };
    // Open every gamepad already attached at startup. Hotplug is handled
    // in `pump` via `ControllerDeviceAdded`.
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
/// internally — callers only see the narrow [`GamepadEvent`]. No-op if
/// [`init`] never succeeded.
///
/// Must run on the thread that called [`init`]; touching the SDL handles
/// from any other thread panics (via `SendWrapper`).
pub fn pump(mut on_event: impl FnMut(GamepadEvent)) {
    let Some(mut pump) = event_pump() else { return };
    let mut guard = GAMEPAD_CONTEXT.lock().unwrap();
    let Some(ctx) = guard.as_mut() else { return };
    while let Some(event) = pump.poll_event() {
        match event {
            SdlEvent::ControllerButtonDown { button, .. } => {
                on_event(GamepadEvent::ButtonDown(Button::from_sdl(button)));
            }
            SdlEvent::ControllerButtonUp { button, .. } => {
                on_event(GamepadEvent::ButtonUp(Button::from_sdl(button)));
            }
            SdlEvent::ControllerAxisMotion { axis, value, .. } => {
                use sdl3::gamepad::Axis as A;
                let axis = match axis {
                    A::LeftX => Axis::LeftX,
                    A::LeftY => Axis::LeftY,
                    A::RightX => Axis::RightX,
                    A::RightY => Axis::RightY,
                    A::TriggerLeft => Axis::TriggerLeft,
                    A::TriggerRight => Axis::TriggerRight,
                };
                on_event(GamepadEvent::AxisMotion {
                    axis,
                    // SDL's raw i16 [-32768, 32767] → [-1, 1]. The sign
                    // convention (stick-up negative) is left untouched.
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

/// RAII exclusive borrow of the global event pump. `!Send` (it holds a
/// `MutexGuard`), so it can't be smuggled off the init thread.
struct EventPumpGuard {
    guard: std::sync::MutexGuard<'static, Option<SendWrapper<EventPump>>>,
}

impl std::ops::Deref for EventPumpGuard {
    type Target = EventPump;
    fn deref(&self) -> &EventPump {
        self.guard.as_ref().unwrap()
    }
}

impl std::ops::DerefMut for EventPumpGuard {
    fn deref_mut(&mut self) -> &mut EventPump {
        self.guard.as_mut().unwrap()
    }
}

fn event_pump() -> Option<EventPumpGuard> {
    let guard = EVENT_PUMP.lock().unwrap();
    guard.as_ref()?;
    Some(EventPumpGuard { guard })
}
