//! Configurable input mapping for the live emulator sessions.
//!
//! - [`PhysicalInput`] describes a single binding source: keyboard
//!   key (iced's physical [`Code`], serialized as its Debug name,
//!   e.g. `"KeyZ"` / `"ArrowLeft"` / `"ShiftLeft"`), gamepad
//!   button, or gamepad axis past a threshold.
//! - [`Mapping`] is the per-mgba-key list of `PhysicalInput`s the
//!   user has assigned (so one mgba key can have multiple
//!   bindings — keyboard *and* controller).
//! - [`HeldState`] tracks what's currently pressed from keyboard +
//!   gamepad event streams. The session main loop combines
//!   `Mapping` + `HeldState` into the joyflags it pushes to mgba.
//!
//! Keyboard bindings are layout-independent: we match on the
//! physical key's [`Code`] rather than the logical character it
//! produces, so a binding placed on the QWERTY `Z` position keeps
//! working on AZERTY (where that physical key types `W`).
//!
//! [`Code`]: iced::keyboard::key::Code

use iced::keyboard::key::{Code, NativeCode, Physical};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Threshold past which an analog axis counts as "pressed" for a
/// d-pad binding. Matches the legacy app's `AXIS_THRESHOLD`
/// (i16 0x4000 → ~0.5 normalized).
pub const AXIS_THRESHOLD: f32 = 0.5;

/// A single binding source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
#[serde(rename_all = "snake_case")]
pub enum PhysicalInput {
    Key(KeyPhysical),
    Button(GamepadButton),
    Axis { axis: GamepadAxis, dir: AxisDir },
}

/// Thin wrapper around iced's [`Physical`] that adds serde
/// support — iced doesn't ship a `serde` feature. Serializes as
/// the `Code` Debug name (`"KeyZ"`, `"ArrowLeft"`, …) for known
/// codes, or `"<Platform>:<n>"` (e.g. `"Windows:42"`) for
/// unidentified scancodes so users can still bind exotic keys
/// we don't have a `Code` for.
///
/// [`Physical`]: iced::keyboard::key::Physical
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeyPhysical(pub Physical);

impl Serialize for KeyPhysical {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        physical_to_string(&self.0).serialize(s)
    }
}

impl<'de> Deserialize<'de> for KeyPhysical {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        string_to_physical(&s)
            .map(KeyPhysical)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown physical key: {s}")))
    }
}

fn physical_to_string(p: &Physical) -> String {
    match p {
        Physical::Code(c) => format!("{c:?}"),
        Physical::Unidentified(NativeCode::Unidentified) => "Unidentified".into(),
        Physical::Unidentified(NativeCode::Android(n)) => format!("Android:{n}"),
        Physical::Unidentified(NativeCode::MacOS(n)) => format!("MacOS:{n}"),
        Physical::Unidentified(NativeCode::Windows(n)) => format!("Windows:{n}"),
        Physical::Unidentified(NativeCode::Xkb(n)) => format!("Xkb:{n}"),
    }
}

fn string_to_physical(s: &str) -> Option<Physical> {
    if s == "Unidentified" {
        return Some(Physical::Unidentified(NativeCode::Unidentified));
    }
    if let Some((platform, n)) = s.split_once(':') {
        let native = match platform {
            "Android" => NativeCode::Android(n.parse().ok()?),
            "MacOS" => NativeCode::MacOS(n.parse().ok()?),
            "Windows" => NativeCode::Windows(n.parse().ok()?),
            "Xkb" => NativeCode::Xkb(n.parse().ok()?),
            _ => return None,
        };
        return Some(Physical::Unidentified(native));
    }
    string_to_code(s).map(Physical::Code)
}

/// Parse a `Code` Debug-name back to the enum. Limited to the
/// subset users would actually bind for a GBA emulator.
fn string_to_code(s: &str) -> Option<Code> {
    Some(match s {
        // Letters
        "KeyA" => Code::KeyA,
        "KeyB" => Code::KeyB,
        "KeyC" => Code::KeyC,
        "KeyD" => Code::KeyD,
        "KeyE" => Code::KeyE,
        "KeyF" => Code::KeyF,
        "KeyG" => Code::KeyG,
        "KeyH" => Code::KeyH,
        "KeyI" => Code::KeyI,
        "KeyJ" => Code::KeyJ,
        "KeyK" => Code::KeyK,
        "KeyL" => Code::KeyL,
        "KeyM" => Code::KeyM,
        "KeyN" => Code::KeyN,
        "KeyO" => Code::KeyO,
        "KeyP" => Code::KeyP,
        "KeyQ" => Code::KeyQ,
        "KeyR" => Code::KeyR,
        "KeyS" => Code::KeyS,
        "KeyT" => Code::KeyT,
        "KeyU" => Code::KeyU,
        "KeyV" => Code::KeyV,
        "KeyW" => Code::KeyW,
        "KeyX" => Code::KeyX,
        "KeyY" => Code::KeyY,
        "KeyZ" => Code::KeyZ,
        // Digits
        "Digit0" => Code::Digit0,
        "Digit1" => Code::Digit1,
        "Digit2" => Code::Digit2,
        "Digit3" => Code::Digit3,
        "Digit4" => Code::Digit4,
        "Digit5" => Code::Digit5,
        "Digit6" => Code::Digit6,
        "Digit7" => Code::Digit7,
        "Digit8" => Code::Digit8,
        "Digit9" => Code::Digit9,
        // Arrows / navigation
        "ArrowLeft" => Code::ArrowLeft,
        "ArrowRight" => Code::ArrowRight,
        "ArrowUp" => Code::ArrowUp,
        "ArrowDown" => Code::ArrowDown,
        "Home" => Code::Home,
        "End" => Code::End,
        "PageUp" => Code::PageUp,
        "PageDown" => Code::PageDown,
        "Insert" => Code::Insert,
        "Delete" => Code::Delete,
        // Modifiers (physical: left/right are distinct)
        "ShiftLeft" => Code::ShiftLeft,
        "ShiftRight" => Code::ShiftRight,
        "ControlLeft" => Code::ControlLeft,
        "ControlRight" => Code::ControlRight,
        "AltLeft" => Code::AltLeft,
        "AltRight" => Code::AltRight,
        "SuperLeft" => Code::SuperLeft,
        "SuperRight" => Code::SuperRight,
        "CapsLock" => Code::CapsLock,
        // Misc
        "Enter" => Code::Enter,
        "Space" => Code::Space,
        "Tab" => Code::Tab,
        "Escape" => Code::Escape,
        "Backspace" => Code::Backspace,
        // Punctuation
        "Backquote" => Code::Backquote,
        "Minus" => Code::Minus,
        "Equal" => Code::Equal,
        "BracketLeft" => Code::BracketLeft,
        "BracketRight" => Code::BracketRight,
        "Backslash" => Code::Backslash,
        "Semicolon" => Code::Semicolon,
        "Quote" => Code::Quote,
        "Comma" => Code::Comma,
        "Period" => Code::Period,
        "Slash" => Code::Slash,
        // Function keys
        "F1" => Code::F1,
        "F2" => Code::F2,
        "F3" => Code::F3,
        "F4" => Code::F4,
        "F5" => Code::F5,
        "F6" => Code::F6,
        "F7" => Code::F7,
        "F8" => Code::F8,
        "F9" => Code::F9,
        "F10" => Code::F10,
        "F11" => Code::F11,
        "F12" => Code::F12,
        // Numpad
        "Numpad0" => Code::Numpad0,
        "Numpad1" => Code::Numpad1,
        "Numpad2" => Code::Numpad2,
        "Numpad3" => Code::Numpad3,
        "Numpad4" => Code::Numpad4,
        "Numpad5" => Code::Numpad5,
        "Numpad6" => Code::Numpad6,
        "Numpad7" => Code::Numpad7,
        "Numpad8" => Code::Numpad8,
        "Numpad9" => Code::Numpad9,
        "NumpadEnter" => Code::NumpadEnter,
        "NumpadAdd" => Code::NumpadAdd,
        "NumpadSubtract" => Code::NumpadSubtract,
        "NumpadMultiply" => Code::NumpadMultiply,
        "NumpadDivide" => Code::NumpadDivide,
        "NumpadDecimal" => Code::NumpadDecimal,
        _ => return None,
    })
}

/// Sign for an analog-axis binding (e.g. "left stick X past
/// negative threshold" = "left").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AxisDir {
    Positive,
    Negative,
}

/// SDL3 gamepad buttons we expose for binding — the full set SDL3
/// can report, mapped 1:1 from [`sdl3::gamepad::Button`] in
/// [`Self::from_sdl3`]. Beyond the standard Xbox/PS
/// face/shoulder/d-pad layout this covers the extras on fancier
/// pads: the `Misc*` share/capture-style buttons, the four back
/// paddles, and the touchpad click. Triggers aren't buttons here —
/// SDL3 reports them as axes, so bind them through
/// [`GamepadAxis::TriggerLeft`] / [`GamepadAxis::TriggerRight`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GamepadButton {
    South,  // A on Xbox, X on PS
    East,   // B on Xbox, Circle on PS
    West,   // X on Xbox, Square on PS
    North,  // Y on Xbox, Triangle on PS
    Select, // Back / Share
    Start,  // Start / Options
    Mode,   // Guide / PS button
    LeftThumb,
    RightThumb,
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

impl GamepadButton {
    pub fn from_sdl3(b: sdl3::gamepad::Button) -> Self {
        use sdl3::gamepad::Button as B;
        match b {
            B::South => Self::South,
            B::East => Self::East,
            B::West => Self::West,
            B::North => Self::North,
            B::Back => Self::Select,
            B::Start => Self::Start,
            B::Guide => Self::Mode,
            B::LeftStick => Self::LeftThumb,
            B::RightStick => Self::RightThumb,
            B::LeftShoulder => Self::LeftShoulder,
            B::RightShoulder => Self::RightShoulder,
            B::DPadUp => Self::DPadUp,
            B::DPadDown => Self::DPadDown,
            B::DPadLeft => Self::DPadLeft,
            B::DPadRight => Self::DPadRight,
            B::Misc1 => Self::Misc1,
            B::Misc2 => Self::Misc2,
            B::Misc3 => Self::Misc3,
            B::Misc4 => Self::Misc4,
            B::Misc5 => Self::Misc5,
            B::Misc6 => Self::Misc6,
            B::RightPaddle1 => Self::RightPaddle1,
            B::LeftPaddle1 => Self::LeftPaddle1,
            B::RightPaddle2 => Self::RightPaddle2,
            B::LeftPaddle2 => Self::LeftPaddle2,
            B::Touchpad => Self::Touchpad,
        }
    }

    /// Fluent key for this button's display label, resolved against
    /// the active locale by [`describe`]. (`Button` itself can't be
    /// localized in isolation — it doesn't carry a `lang`.)
    pub fn label_key(&self) -> &'static str {
        match self {
            Self::South => "input-gamepad-south",
            Self::East => "input-gamepad-east",
            Self::West => "input-gamepad-west",
            Self::North => "input-gamepad-north",
            Self::Select => "input-gamepad-select",
            Self::Start => "input-gamepad-start",
            Self::Mode => "input-gamepad-mode",
            Self::LeftThumb => "input-gamepad-left-thumb",
            Self::RightThumb => "input-gamepad-right-thumb",
            Self::LeftShoulder => "input-gamepad-left-shoulder",
            Self::RightShoulder => "input-gamepad-right-shoulder",
            Self::DPadUp => "input-gamepad-dpad-up",
            Self::DPadDown => "input-gamepad-dpad-down",
            Self::DPadLeft => "input-gamepad-dpad-left",
            Self::DPadRight => "input-gamepad-dpad-right",
            Self::Misc1 => "input-gamepad-misc1",
            Self::Misc2 => "input-gamepad-misc2",
            Self::Misc3 => "input-gamepad-misc3",
            Self::Misc4 => "input-gamepad-misc4",
            Self::Misc5 => "input-gamepad-misc5",
            Self::Misc6 => "input-gamepad-misc6",
            Self::RightPaddle1 => "input-gamepad-right-paddle1",
            Self::LeftPaddle1 => "input-gamepad-left-paddle1",
            Self::RightPaddle2 => "input-gamepad-right-paddle2",
            Self::LeftPaddle2 => "input-gamepad-left-paddle2",
            Self::Touchpad => "input-gamepad-touchpad",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GamepadAxis {
    LeftStickX,
    LeftStickY,
    RightStickX,
    RightStickY,
    TriggerLeft,
    TriggerRight,
}

impl GamepadAxis {
    /// Fluent key for this axis's display label, resolved against
    /// the active locale by [`describe`]. The `+`/`−` direction
    /// sign is prepended separately by the caller.
    pub fn label_key(&self) -> &'static str {
        match self {
            Self::LeftStickX => "input-gamepad-axis-left-stick-x",
            Self::LeftStickY => "input-gamepad-axis-left-stick-y",
            Self::RightStickX => "input-gamepad-axis-right-stick-x",
            Self::RightStickY => "input-gamepad-axis-right-stick-y",
            Self::TriggerLeft => "input-gamepad-axis-trigger-left",
            Self::TriggerRight => "input-gamepad-axis-trigger-right",
        }
    }
}

/// Per-mgba-key list of `PhysicalInput`. Each key can have
/// multiple bindings (kbd + gamepad simultaneously); pressing
/// any one of them counts as the mgba key being held.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Mapping {
    pub up: Vec<PhysicalInput>,
    pub down: Vec<PhysicalInput>,
    pub left: Vec<PhysicalInput>,
    pub right: Vec<PhysicalInput>,
    pub a: Vec<PhysicalInput>,
    pub b: Vec<PhysicalInput>,
    pub l: Vec<PhysicalInput>,
    pub r: Vec<PhysicalInput>,
    pub start: Vec<PhysicalInput>,
    pub select: Vec<PhysicalInput>,
    pub speed_up: Vec<PhysicalInput>,
}

impl Default for Mapping {
    fn default() -> Self {
        // Matches the legacy app's defaults: arrows + WASD-ish
        // for L/R, Z/X for A/B, Enter/Space for Start/Select.
        // Speed-up = LShift. Controller defaults track the
        // legacy app's Xbox-layout bindings.
        let key = |c| PhysicalInput::Key(KeyPhysical(Physical::Code(c)));
        let btn = PhysicalInput::Button;
        let axis = |axis, dir| PhysicalInput::Axis { axis, dir };
        Self {
            up: vec![
                key(Code::ArrowUp),
                btn(GamepadButton::DPadUp),
                axis(GamepadAxis::LeftStickY, AxisDir::Negative),
            ],
            down: vec![
                key(Code::ArrowDown),
                btn(GamepadButton::DPadDown),
                axis(GamepadAxis::LeftStickY, AxisDir::Positive),
            ],
            left: vec![
                key(Code::ArrowLeft),
                btn(GamepadButton::DPadLeft),
                axis(GamepadAxis::LeftStickX, AxisDir::Negative),
            ],
            right: vec![
                key(Code::ArrowRight),
                btn(GamepadButton::DPadRight),
                axis(GamepadAxis::LeftStickX, AxisDir::Positive),
            ],
            a: vec![key(Code::KeyZ), btn(GamepadButton::South)],
            b: vec![key(Code::KeyX), btn(GamepadButton::East)],
            l: vec![key(Code::KeyA), btn(GamepadButton::LeftShoulder)],
            r: vec![key(Code::KeyS), btn(GamepadButton::RightShoulder)],
            start: vec![key(Code::Enter), btn(GamepadButton::Start)],
            select: vec![key(Code::Space), btn(GamepadButton::Select)],
            speed_up: vec![key(Code::ShiftLeft)],
        }
    }
}

impl Mapping {
    /// The binding list for one mapped key. Used by the settings
    /// editor's console view to look up each key it draws.
    pub fn slot(&self, key: MappedKey) -> &Vec<PhysicalInput> {
        match key {
            MappedKey::Up => &self.up,
            MappedKey::Down => &self.down,
            MappedKey::Left => &self.left,
            MappedKey::Right => &self.right,
            MappedKey::A => &self.a,
            MappedKey::B => &self.b,
            MappedKey::L => &self.l,
            MappedKey::R => &self.r,
            MappedKey::Start => &self.start,
            MappedKey::Select => &self.select,
            MappedKey::SpeedUp => &self.speed_up,
        }
    }

    pub fn slot_mut(&mut self, key: MappedKey) -> &mut Vec<PhysicalInput> {
        match key {
            MappedKey::Up => &mut self.up,
            MappedKey::Down => &mut self.down,
            MappedKey::Left => &mut self.left,
            MappedKey::Right => &mut self.right,
            MappedKey::A => &mut self.a,
            MappedKey::B => &mut self.b,
            MappedKey::L => &mut self.l,
            MappedKey::R => &mut self.r,
            MappedKey::Start => &mut self.start,
            MappedKey::Select => &mut self.select,
            MappedKey::SpeedUp => &mut self.speed_up,
        }
    }

    /// Compute the mgba joyflag bitmask for the supplied held
    /// state. Speed-up isn't an mgba bit; check it separately via
    /// [`Self::speed_up_held`].
    pub fn to_mgba_keys(&self, state: &HeldState) -> u32 {
        use mgba::input::keys;
        let bit_if = |slot: &Vec<PhysicalInput>, bit: u32| -> u32 {
            if slot.iter().any(|p| state.is_active(p)) {
                bit
            } else {
                0
            }
        };
        bit_if(&self.up, keys::UP)
            | bit_if(&self.down, keys::DOWN)
            | bit_if(&self.left, keys::LEFT)
            | bit_if(&self.right, keys::RIGHT)
            | bit_if(&self.a, keys::A)
            | bit_if(&self.b, keys::B)
            | bit_if(&self.l, keys::L)
            | bit_if(&self.r, keys::R)
            | bit_if(&self.start, keys::START)
            | bit_if(&self.select, keys::SELECT)
    }

    pub fn speed_up_held(&self, state: &HeldState) -> bool {
        self.speed_up.iter().any(|p| state.is_active(p))
    }
}

/// The mgba keys the user can rebind. Drives the settings UI
/// layout + the per-key add/remove flow.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MappedKey {
    Up,
    Down,
    Left,
    Right,
    A,
    B,
    L,
    R,
    Start,
    Select,
    SpeedUp,
}

/// Atomic input event fed to the held-state tracker. Carries the raw
/// key/button/axis info so both consumers — the session's joyflag
/// pipeline and the input settings pane's live binding highlight —
/// share one normalized stream (see
/// [`crate::platform::input_capture::Input::to_event`]).
#[derive(Debug, Clone)]
pub enum Event {
    Key {
        physical: Physical,
        pressed: bool,
    },
    Button {
        button: GamepadButton,
        pressed: bool,
    },
    Axis {
        axis: GamepadAxis,
        value: f32,
    },
    /// Controller dropped — clear all gamepad state so
    /// disconnected buttons don't read as still-held.
    GamepadDisconnected,
}

/// Live held-input state combined from keyboard + every connected
/// gamepad. The session loop updates this on every key/gamepad
/// event then asks the Mapping to compute the resulting joyflags.
#[derive(Default)]
pub struct HeldState {
    keys: HashSet<Physical>,
    buttons: HashSet<GamepadButton>,
    /// Per-axis last-known normalized value in [-1, 1]. Bindings
    /// trigger when |value| crosses [`AXIS_THRESHOLD`].
    axes: HashMap<GamepadAxis, f32>,
}

impl HeldState {
    /// Fold one event into the held sets.
    pub fn apply(&mut self, ev: &Event) {
        match *ev {
            Event::Key { physical, pressed } => self.set_key(physical, pressed),
            Event::Button { button, pressed } => self.set_button(button, pressed),
            Event::Axis { axis, value } => self.set_axis(axis, value),
            Event::GamepadDisconnected => self.clear_gamepad(),
        }
    }

    pub fn set_key(&mut self, physical: Physical, pressed: bool) {
        if pressed {
            self.keys.insert(physical);
        } else {
            self.keys.remove(&physical);
        }
    }

    /// Whether `physical` is currently held. Lets edge-triggered keybinds
    /// (e.g. spacebar play/pause) tell a fresh press from OS key-repeat.
    pub fn is_key_held(&self, physical: &Physical) -> bool {
        self.keys.contains(physical)
    }

    pub fn set_button(&mut self, b: GamepadButton, pressed: bool) {
        if pressed {
            self.buttons.insert(b);
        } else {
            self.buttons.remove(&b);
        }
    }

    pub fn set_axis(&mut self, a: GamepadAxis, value: f32) {
        self.axes.insert(a, value);
    }

    /// Clear gamepad state — call when a controller disconnects so
    /// stuck-pressed buttons don't leak across reconnects.
    pub fn clear_gamepad(&mut self) {
        self.buttons.clear();
        self.axes.clear();
    }

    pub fn is_active(&self, p: &PhysicalInput) -> bool {
        match p {
            PhysicalInput::Key(c) => self.keys.contains(&c.0),
            PhysicalInput::Button(b) => self.buttons.contains(b),
            PhysicalInput::Axis { axis, dir } => {
                let v = self.axes.get(axis).copied().unwrap_or(0.0);
                match dir {
                    AxisDir::Positive => v > AXIS_THRESHOLD,
                    AxisDir::Negative => v < -AXIS_THRESHOLD,
                }
            }
        }
    }
}

/// What kind of physical source produced a binding. Used by the
/// settings UI to pick the right Lucide glyph for the chip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DescribeKind {
    Keyboard,
    Gamepad,
}

/// Pretty-print a binding for the settings UI. Returns the source
/// kind (for the chip's Lucide glyph) and a label. Gamepad button
/// names are localized via the active locale; keyboard key names
/// are physical [`Code`] identifiers and stay as-is.
pub fn describe(lang: &unic_langid::LanguageIdentifier, p: &PhysicalInput) -> (DescribeKind, String) {
    match p {
        PhysicalInput::Key(c) => (DescribeKind::Keyboard, physical_to_string(&c.0)),
        PhysicalInput::Button(b) => (DescribeKind::Gamepad, crate::i18n::t(lang, b.label_key())),
        PhysicalInput::Axis { axis, dir } => {
            let sign = match dir {
                AxisDir::Positive => "+",
                AxisDir::Negative => "−",
            };
            let name = crate::i18n::t(lang, axis.label_key());
            (DescribeKind::Gamepad, format!("{sign}{name}"))
        }
    }
}
