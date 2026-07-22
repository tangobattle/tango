//! Configurable input mapping for the live emulator sessions.
//!
//! - [`PhysicalInput`] describes a single binding source: keyboard key
//!   (the DOM [`KeyboardEvent.code`] string, e.g. `"KeyZ"` /
//!   `"ArrowLeft"` / `"ShiftLeft"`), gamepad button, or gamepad axis
//!   past a threshold.
//! - [`Mapping`] is the per-mgba-key list of `PhysicalInput`s the
//!   user has assigned (so one mgba key can have multiple
//!   bindings — keyboard *and* controller).
//! - [`HeldState`] tracks what's currently pressed from keyboard +
//!   gamepad. The runtime pump combines `Mapping` + `HeldState` into
//!   the joyflags it pushes to mgba.
//!
//! Keyboard bindings are layout-independent: `KeyboardEvent.code`
//! names the physical key position rather than the logical character
//! it produces, so a binding placed on the QWERTY `Z` position keeps
//! working on AZERTY (where that physical key types `W`). The serde
//! format is unchanged from the retired native client, which used the
//! same names via iced's physical `Code` — existing mappings load.
//!
//! [`KeyboardEvent.code`]: https://developer.mozilla.org/docs/Web/API/KeyboardEvent/code

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Threshold past which an analog axis counts as "pressed" for a
/// d-pad binding. Matches the legacy app's `AXIS_THRESHOLD`
/// (i16 0x4000 → ~0.5 normalized).
pub const AXIS_THRESHOLD: f32 = 0.5;

/// A single binding source.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
#[serde(rename_all = "snake_case")]
pub enum PhysicalInput {
    Key(KeyPhysical),
    Button(GamepadButton),
    Axis { axis: GamepadAxis, dir: AxisDir },
}

/// A physical key position: the DOM `KeyboardEvent.code` string,
/// verbatim. Serializes as itself (`"KeyZ"`, `"ArrowLeft"`, …).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KeyPhysical(pub String);

impl From<&str> for KeyPhysical {
    fn from(code: &str) -> Self {
        KeyPhysical(code.to_owned())
    }
}

/// Sign for an analog-axis binding (e.g. "left stick X past
/// negative threshold" = "left").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AxisDir {
    Positive,
    Negative,
}

/// Gamepad buttons we expose for binding. The names (and their serde
/// spellings) carry over from the native client's SDL3 set so saved
/// mappings stay loadable; the browser's "standard" gamepad mapping
/// populates the common subset (see `platform::gamepad`), and the
/// `Misc*`/paddle variants simply never fire there.
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
    /// Map from SDL3's button enum (native backend). 1:1 — the enum
    /// was copied from the desktop client's SDL3 set to begin with.
    #[cfg(not(target_arch = "wasm32"))]
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

    /// Display label for the settings UI.
    pub fn label(&self) -> &'static str {
        match self {
            Self::South => "A/Cross",
            Self::East => "B/Circle",
            Self::West => "X/Square",
            Self::North => "Y/Triangle",
            Self::Select => "Back/Share",
            Self::Start => "Start",
            Self::Mode => "Guide",
            Self::LeftThumb => "Left stick click",
            Self::RightThumb => "Right stick click",
            Self::LeftShoulder => "LB",
            Self::RightShoulder => "RB",
            Self::DPadUp => "D-pad up",
            Self::DPadDown => "D-pad down",
            Self::DPadLeft => "D-pad left",
            Self::DPadRight => "D-pad right",
            Self::Misc1 => "Misc 1",
            Self::Misc2 => "Misc 2",
            Self::Misc3 => "Misc 3",
            Self::Misc4 => "Misc 4",
            Self::Misc5 => "Misc 5",
            Self::Misc6 => "Misc 6",
            Self::RightPaddle1 => "Right paddle 1",
            Self::LeftPaddle1 => "Left paddle 1",
            Self::RightPaddle2 => "Right paddle 2",
            Self::LeftPaddle2 => "Left paddle 2",
            Self::Touchpad => "Touchpad",
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
    /// Display label for the settings UI. The `+`/`−` direction sign
    /// is prepended separately by the caller.
    pub fn label(&self) -> &'static str {
        match self {
            Self::LeftStickX => "Left stick X",
            Self::LeftStickY => "Left stick Y",
            Self::RightStickX => "Right stick X",
            Self::RightStickY => "Right stick Y",
            Self::TriggerLeft => "Left trigger",
            Self::TriggerRight => "Right trigger",
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
        let key = |c: &str| PhysicalInput::Key(c.into());
        let btn = PhysicalInput::Button;
        let axis = |axis, dir| PhysicalInput::Axis { axis, dir };
        Self {
            up: vec![
                key("ArrowUp"),
                btn(GamepadButton::DPadUp),
                axis(GamepadAxis::LeftStickY, AxisDir::Negative),
            ],
            down: vec![
                key("ArrowDown"),
                btn(GamepadButton::DPadDown),
                axis(GamepadAxis::LeftStickY, AxisDir::Positive),
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
            a: vec![key("KeyZ"), btn(GamepadButton::South)],
            b: vec![key("KeyX"), btn(GamepadButton::East)],
            l: vec![key("KeyA"), btn(GamepadButton::LeftShoulder)],
            r: vec![key("KeyS"), btn(GamepadButton::RightShoulder)],
            start: vec![key("Enter"), btn(GamepadButton::Start)],
            select: vec![key("Space"), btn(GamepadButton::Select)],
            speed_up: vec![key("ShiftLeft")],
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

    /// Whether any binding uses this key code — the pump's
    /// preventDefault rule, so bound keys don't scroll the page while
    /// unbound ones keep their browser behavior.
    pub fn binds_code(&self, code: &str) -> bool {
        [
            &self.up,
            &self.down,
            &self.left,
            &self.right,
            &self.a,
            &self.b,
            &self.l,
            &self.r,
            &self.start,
            &self.select,
            &self.speed_up,
        ]
        .iter()
        .any(|slot| {
            slot.iter()
                .any(|p| matches!(p, PhysicalInput::Key(KeyPhysical(c)) if c == code))
        })
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

/// Every gamepad-sourced binding the capture flow can observe: the
/// buttons the browser's "standard" mapping produces (see
/// `platform::gamepad`), plus both directions of every axis. Keyboard
/// capture doesn't enumerate — it takes whatever code the event carries.
pub fn gamepad_candidates() -> Vec<PhysicalInput> {
    const BUTTONS: [GamepadButton; 16] = [
        GamepadButton::South,
        GamepadButton::East,
        GamepadButton::West,
        GamepadButton::North,
        GamepadButton::Select,
        GamepadButton::Start,
        GamepadButton::Mode,
        GamepadButton::LeftThumb,
        GamepadButton::RightThumb,
        GamepadButton::LeftShoulder,
        GamepadButton::RightShoulder,
        GamepadButton::DPadUp,
        GamepadButton::DPadDown,
        GamepadButton::DPadLeft,
        GamepadButton::DPadRight,
        GamepadButton::Touchpad,
    ];
    const AXES: [GamepadAxis; 6] = [
        GamepadAxis::LeftStickX,
        GamepadAxis::LeftStickY,
        GamepadAxis::RightStickX,
        GamepadAxis::RightStickY,
        GamepadAxis::TriggerLeft,
        GamepadAxis::TriggerRight,
    ];
    BUTTONS
        .into_iter()
        .map(PhysicalInput::Button)
        .chain(AXES.into_iter().flat_map(|axis| {
            [AxisDir::Positive, AxisDir::Negative]
                .into_iter()
                .map(move |dir| PhysicalInput::Axis { axis, dir })
        }))
        .collect()
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
/// gamepad. The runtime pump updates this from key events and the
/// per-pump gamepad snapshot, then asks the Mapping to compute the
/// resulting joyflags.
#[derive(Default)]
pub struct HeldState {
    keys: HashSet<KeyPhysical>,
    buttons: HashSet<GamepadButton>,
    /// Per-axis last-known normalized value in [-1, 1]. Bindings
    /// trigger when |value| crosses [`AXIS_THRESHOLD`].
    axes: HashMap<GamepadAxis, f32>,
}

impl HeldState {
    /// Forget every held keyboard key. Called when the tab loses focus
    /// or visibility: the matching keyup lands in another tab, so a
    /// key held across the switch would otherwise stay "down" forever
    /// (a held fast-forward would silently pin the tab at 3× CPU).
    /// Gamepad state is exempt — it's re-polled whole every pump.
    /// (Unwired on native: Blitz 0.7.9 delivers no focus/blur events;
    /// keyups arrive through the window regardless of focus loss.)
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub fn release_keys(&mut self) {
        self.keys.clear();
    }

    pub fn set_key(&mut self, code: &str, pressed: bool) {
        if pressed {
            self.keys.insert(code.into());
        } else {
            self.keys.remove(&KeyPhysical(code.to_owned()));
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

    /// Clear gamepad state — the pump rebuilds it from a fresh
    /// `getGamepads()` snapshot every tick, and a disconnected pad
    /// must not read as still-held.
    pub fn clear_gamepad(&mut self) {
        self.buttons.clear();
        self.axes.clear();
    }

    pub fn is_active(&self, p: &PhysicalInput) -> bool {
        match p {
            PhysicalInput::Key(c) => self.keys.contains(c),
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
/// settings UI to pick the right glyph for the chip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DescribeKind {
    Keyboard,
    Gamepad,
}

/// Pretty-print a binding for the settings UI. Returns the source
/// kind (keyboard vs gamepad) and a label. Keyboard key names are
/// the DOM `code` identifiers.
pub fn describe(p: &PhysicalInput) -> (DescribeKind, String) {
    match p {
        PhysicalInput::Key(c) => (DescribeKind::Keyboard, c.0.clone()),
        PhysicalInput::Button(b) => (DescribeKind::Gamepad, b.label().to_string()),
        PhysicalInput::Axis { axis, dir } => {
            let sign = match dir {
                AxisDir::Positive => "+",
                AxisDir::Negative => "−",
            };
            (DescribeKind::Gamepad, format!("{sign}{}", axis.label()))
        }
    }
}
