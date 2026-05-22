//! Configurable input mapping for the live emulator sessions.
//!
//! - [`PhysicalInput`] describes a single binding source: keyboard
//!   key (serialized via a small string-keyed subset of iced's
//!   `keyboard::Key`), gamepad button, or gamepad axis past a
//!   threshold.
//! - [`Mapping`] is the per-mgba-key list of `PhysicalInput`s the
//!   user has assigned (so one mgba key can have multiple
//!   bindings — keyboard *and* controller).
//! - [`HeldState`] tracks what's currently pressed from keyboard +
//!   gamepad event streams. The session main loop combines
//!   `Mapping` + `HeldState` into the joyflags it pushes to mgba.
//!
//! Legacy parity: same `PhysicalInput` shape as
//! `tango/src/input.rs`, same default bindings.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Threshold past which an analog axis counts as "pressed" for a
/// d-pad binding. Matches the legacy app's `AXIS_THRESHOLD`
/// (i16 0x4000 → ~0.5 normalized).
pub const AXIS_THRESHOLD: f32 = 0.5;

/// A single binding source. Keyboard keys are stored as strings
/// so the on-disk config stays human-readable + portable across
/// iced versions; gamepad buttons + axes mirror the SDL3
/// `gamepad::Button` / `gamepad::Axis` enums via [`GamepadButton`]
/// / [`GamepadAxis`].
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
#[serde(rename_all = "snake_case")]
pub enum PhysicalInput {
    Key(KeyId),
    Button(GamepadButton),
    Axis { axis: GamepadAxis, dir: AxisDir },
}

/// Serializable string-keyed wrapper around iced's keyboard key.
/// Round-tripped via a tiny manual table covering everything you'd
/// realistically bind for a GBA emulator — letters, arrows,
/// enter/space/shift/etc. Anything outside the table parses back
/// to [`KeyId::Unbindable`] (kept around so a stale binding
/// doesn't disappear from the config silently).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct KeyId(pub String);

impl KeyId {
    /// Translate from an iced runtime key event. Returns None for
    /// keys we have no string representation for (the user has to
    /// pick another one to bind).
    pub fn from_iced(key: &iced::keyboard::Key) -> Option<Self> {
        let s = key_to_string(key)?;
        Some(KeyId(s))
    }

    /// Human-readable label for the settings UI.
    pub fn label(&self) -> &str {
        &self.0
    }
}

/// Render an iced key as the lowercase string we store on disk.
/// Falls through to None for stuff like Meta/Compose that doesn't
/// make sense as a game-control binding.
fn key_to_string(key: &iced::keyboard::Key) -> Option<String> {
    use iced::keyboard::key::{Key, Named};
    Some(match key {
        Key::Named(n) => match n {
            Named::ArrowLeft => "ArrowLeft".into(),
            Named::ArrowRight => "ArrowRight".into(),
            Named::ArrowUp => "ArrowUp".into(),
            Named::ArrowDown => "ArrowDown".into(),
            Named::Enter => "Enter".into(),
            Named::Space => "Space".into(),
            Named::Tab => "Tab".into(),
            Named::Escape => "Escape".into(),
            Named::Backspace => "Backspace".into(),
            Named::Shift => "Shift".into(),
            Named::Control => "Control".into(),
            Named::Alt => "Alt".into(),
            Named::CapsLock => "CapsLock".into(),
            Named::Home => "Home".into(),
            Named::End => "End".into(),
            Named::PageUp => "PageUp".into(),
            Named::PageDown => "PageDown".into(),
            Named::Insert => "Insert".into(),
            Named::Delete => "Delete".into(),
            _ => return None,
        },
        Key::Character(c) => c.to_lowercase(),
        Key::Unidentified => return None,
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

/// Subset of SDL3 gamepad buttons we expose for binding. We don't
/// expose every button SDL3 reports — just the standard Xbox/PS
/// layout, since rebinding to esoteric paddle/touchpad buttons
/// isn't useful here. `LeftTrigger` / `RightTrigger` are retained
/// for on-disk config back-compat with the previous gilrs-era
/// builds but never fire from SDL3 — SDL3 only reports triggers
/// as axes (`TriggerLeft` / `TriggerRight`). Rebind triggers as
/// axes if needed.
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
    LeftTrigger, // legacy digital trigger pull (gilrs-era); SDL3 reports as axis
    RightTrigger,
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
}

impl GamepadButton {
    pub fn from_sdl3(b: sdl3::gamepad::Button) -> Option<Self> {
        use sdl3::gamepad::Button as B;
        Some(match b {
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
            _ => return None,
        })
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::South => "Button A",
            Self::East => "Button B",
            Self::West => "Button X",
            Self::North => "Button Y",
            Self::Select => "Select",
            Self::Start => "Start",
            Self::Mode => "Guide",
            Self::LeftThumb => "Left Stick",
            Self::RightThumb => "Right Stick",
            Self::LeftShoulder => "LB",
            Self::RightShoulder => "RB",
            Self::LeftTrigger => "LT",
            Self::RightTrigger => "RT",
            Self::DPadUp => "D-Pad Up",
            Self::DPadDown => "D-Pad Down",
            Self::DPadLeft => "D-Pad Left",
            Self::DPadRight => "D-Pad Right",
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
        let key = |s: &str| PhysicalInput::Key(KeyId(s.to_string()));
        let btn = |b| PhysicalInput::Button(b);
        let axis = |axis, dir| PhysicalInput::Axis { axis, dir };
        Self {
            up: vec![
                key("ArrowUp"),
                btn(GamepadButton::DPadUp),
                axis(GamepadAxis::LeftStickY, AxisDir::Positive),
            ],
            down: vec![
                key("ArrowDown"),
                btn(GamepadButton::DPadDown),
                axis(GamepadAxis::LeftStickY, AxisDir::Negative),
            ],
            left: vec![
                key("ArrowLeft"),
                btn(GamepadButton::DPadLeft),
                axis(GamepadAxis::LeftStickX, AxisDir::Negative),
            ],
            right: vec![
                key("ArrowRight"),
                btn(GamepadButton::DPadRight),
                axis(GamepadAxis::LeftStickX, AxisDir::Positive),
            ],
            a: vec![key("z"), btn(GamepadButton::South)],
            b: vec![key("x"), btn(GamepadButton::East)],
            l: vec![key("a"), btn(GamepadButton::LeftShoulder)],
            r: vec![key("s"), btn(GamepadButton::RightShoulder)],
            start: vec![key("Enter"), btn(GamepadButton::Start)],
            select: vec![key("Space"), btn(GamepadButton::Select)],
            speed_up: vec![key("Shift")],
        }
    }
}

impl Mapping {
    /// Iterate every binding slot. Used by the settings editor
    /// to render the per-key tables.
    pub fn slots(&self) -> [(MappedKey, &Vec<PhysicalInput>); 11] {
        [
            (MappedKey::Up, &self.up),
            (MappedKey::Down, &self.down),
            (MappedKey::Left, &self.left),
            (MappedKey::Right, &self.right),
            (MappedKey::A, &self.a),
            (MappedKey::B, &self.b),
            (MappedKey::L, &self.l),
            (MappedKey::R, &self.r),
            (MappedKey::Start, &self.start),
            (MappedKey::Select, &self.select),
            (MappedKey::SpeedUp, &self.speed_up),
        ]
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

/// Live held-input state combined from keyboard + every connected
/// gamepad. The session loop updates this on every key/gamepad
/// event then asks the Mapping to compute the resulting joyflags.
#[derive(Default)]
pub struct HeldState {
    keys: HashSet<String>,
    buttons: HashSet<GamepadButton>,
    /// Per-axis last-known normalized value in [-1, 1]. Bindings
    /// trigger when |value| crosses [`AXIS_THRESHOLD`].
    axes: HashMap<GamepadAxis, f32>,
}

impl HeldState {
    pub fn set_key(&mut self, key: &iced::keyboard::Key, pressed: bool) {
        let Some(s) = key_to_string(key) else {
            return;
        };
        if pressed {
            self.keys.insert(s);
        } else {
            self.keys.remove(&s);
        }
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
            PhysicalInput::Key(k) => self.keys.contains(&k.0),
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
/// kind (for the chip's Lucide glyph) and a plain-text label.
pub fn describe(p: &PhysicalInput) -> (DescribeKind, String) {
    match p {
        PhysicalInput::Key(k) => (DescribeKind::Keyboard, k.label().to_string()),
        PhysicalInput::Button(b) => (DescribeKind::Gamepad, b.label().to_string()),
        PhysicalInput::Axis { axis, dir } => {
            let sign = match dir {
                AxisDir::Positive => "+",
                AxisDir::Negative => "−",
            };
            let name = match axis {
                GamepadAxis::LeftStickX => "Left Stick X",
                GamepadAxis::LeftStickY => "Left Stick Y",
                GamepadAxis::RightStickX => "Right Stick X",
                GamepadAxis::RightStickY => "Right Stick Y",
            };
            (DescribeKind::Gamepad, format!("{sign}{name}"))
        }
    }
}
