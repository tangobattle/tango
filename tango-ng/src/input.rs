//! Configurable keyboard + gamepad → GBA joyflag mapping.
//!
//! - [`Binding`] is one physical source: a keyboard key (stored as a
//!   stable name derived from the Slint key-event char, see
//!   [`key_name`]), a gilrs gamepad button, or a gamepad axis past
//!   [`AXIS_THRESHOLD`] in one direction. Gamepad buttons/axes are
//!   stored as their gilrs Debug names ("South", "LeftStickX", …) so
//!   the held-state lookups below are plain string matches — nothing
//!   ever needs parsing back into gilrs types.
//! - [`Mapping`] is the per-GBA-key list of bindings the user has
//!   assigned (one key can hold keyboard *and* controller bindings),
//!   plus the speed-up (fast-forward) slot. It serializes as a whole
//!   into the config (`config.rs` `input_mapping`).
//! - [`Held`] tracks what's currently pressed, fed by the session
//!   FocusScope's key events and the frame timer's gilrs poll. The
//!   main loop combines `Mapping` + `Held` into the joyflags it
//!   pushes to the active session ([`Mapping::to_joyflags`]).
//!
//! The layout mirrors tango's `src/input.rs` (Mapping / HeldState /
//! describe); the physical-key representation differs because Slint
//! reports logical key text rather than iced's physical scancodes.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Threshold past which an analog axis counts as "pressed" for a
/// d-pad binding, in gilrs's normalized [-1, 1] range. Matches
/// tango's `AXIS_THRESHOLD` (i16 0x4000 → ~0.5).
pub const AXIS_THRESHOLD: f32 = 0.5;

/// A single binding source. Serialized like tango's `PhysicalInput`
/// (`{"kind": "key", "value": "Z"}`, `{"kind": "axis", "value":
/// {"axis": "LeftStickY", "dir": 1}}`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
#[serde(rename_all = "snake_case")]
pub enum Binding {
    /// Keyboard key, by the stable name [`key_name`] produces.
    Key(String),
    /// Gamepad button, by its gilrs Debug name (e.g. "South").
    Button(String),
    /// Gamepad axis past [`AXIS_THRESHOLD`]; `dir` is the sign of the
    /// triggering direction (+1 / -1). gilrs convention: positive =
    /// up / right.
    Axis { axis: String, dir: i8 },
}

/// Canonical stable name for one Slint key-event char, or `None` for
/// keys we don't support binding. Slint delivers special keys as
/// single control / private-use-area chars (`slint::platform::Key`);
/// letters arrive in whatever case Shift produced, so they normalize
/// to uppercase to keep press/release symmetric. Escape is
/// deliberately unmapped — it's hardcoded to end-session /
/// cancel-capture in main.rs.
pub fn key_name(c: char) -> Option<String> {
    use slint::platform::Key;
    if c.is_alphanumeric() {
        // Letters/digits, uppercased (multi-char expansions are fine —
        // both press and release go through the same normalization).
        let mut s = String::new();
        s.extend(c.to_uppercase());
        return Some(s);
    }
    let name = if c == char::from(Key::Return) {
        "Return"
    } else if c == char::from(Key::Space) {
        "Space"
    } else if c == char::from(Key::Tab) {
        "Tab"
    } else if c == char::from(Key::Backtab) {
        "Backtab"
    } else if c == char::from(Key::Backspace) {
        "Backspace"
    } else if c == char::from(Key::Delete) {
        "Delete"
    } else if c == char::from(Key::Shift) {
        "Shift"
    } else if c == char::from(Key::ShiftR) {
        "RightShift"
    } else if c == char::from(Key::Control) {
        "Control"
    } else if c == char::from(Key::ControlR) {
        "RightControl"
    } else if c == char::from(Key::Alt) {
        "Alt"
    } else if c == char::from(Key::AltGr) {
        "AltGr"
    } else if c == char::from(Key::Meta) {
        "Meta"
    } else if c == char::from(Key::MetaR) {
        "RightMeta"
    } else if c == char::from(Key::CapsLock) {
        "CapsLock"
    } else if c == char::from(Key::UpArrow) {
        "Up"
    } else if c == char::from(Key::DownArrow) {
        "Down"
    } else if c == char::from(Key::LeftArrow) {
        "Left"
    } else if c == char::from(Key::RightArrow) {
        "Right"
    } else if c == char::from(Key::Insert) {
        "Insert"
    } else if c == char::from(Key::Home) {
        "Home"
    } else if c == char::from(Key::End) {
        "End"
    } else if c == char::from(Key::PageUp) {
        "PageUp"
    } else if c == char::from(Key::PageDown) {
        "PageDown"
    } else if c == char::from(Key::F1) {
        "F1"
    } else if c == char::from(Key::F2) {
        "F2"
    } else if c == char::from(Key::F3) {
        "F3"
    } else if c == char::from(Key::F4) {
        "F4"
    } else if c == char::from(Key::F5) {
        "F5"
    } else if c == char::from(Key::F6) {
        "F6"
    } else if c == char::from(Key::F7) {
        "F7"
    } else if c == char::from(Key::F8) {
        "F8"
    } else if c == char::from(Key::F9) {
        "F9"
    } else if c == char::from(Key::F10) {
        "F10"
    } else if c == char::from(Key::F11) {
        "F11"
    } else if c == char::from(Key::F12) {
        "F12"
    } else if c.is_ascii_graphic() {
        // Punctuation: the produced character is its own stable name.
        return Some(c.to_string());
    } else {
        return None;
    };
    Some(name.to_string())
}

/// Display label for a stored keyboard-key name — mostly the name
/// itself, with the couple of cases where friendlier copy exists.
pub fn key_label(name: &str) -> String {
    match name {
        "Return" => "Enter".to_string(),
        "Up" => "↑".to_string(),
        "Down" => "↓".to_string(),
        "Left" => "←".to_string(),
        "Right" => "→".to_string(),
        other => other.to_string(),
    }
}

/// Stable name for a gilrs button (its Debug name), or `None` for
/// `Unknown` — unidentified buttons can't round-trip a binding.
pub fn button_name(b: gilrs::Button) -> Option<String> {
    (b != gilrs::Button::Unknown).then(|| format!("{b:?}"))
}

/// Stable name for a gilrs axis (its Debug name), or `None` for
/// `Unknown`.
pub fn axis_name(a: gilrs::Axis) -> Option<String> {
    (a != gilrs::Axis::Unknown).then(|| format!("{a:?}"))
}

/// Localized display label for a stored gamepad-button name, using
/// tango's `input-gamepad-*` Fluent keys. gilrs's `LeftTrigger` /
/// `RightTrigger` are the shoulder bumpers (LB/RB); `*Trigger2` are
/// the analog triggers. Names without a key (`C`, `Z`) stay raw.
fn button_label(lang: &unic_langid::LanguageIdentifier, name: &str) -> String {
    match name {
        "South" => crate::t!(lang, "input-gamepad-south"),
        "East" => crate::t!(lang, "input-gamepad-east"),
        "West" => crate::t!(lang, "input-gamepad-west"),
        "North" => crate::t!(lang, "input-gamepad-north"),
        "Select" => crate::t!(lang, "input-gamepad-select"),
        "Start" => crate::t!(lang, "input-gamepad-start"),
        "Mode" => crate::t!(lang, "input-gamepad-mode"),
        "LeftThumb" => crate::t!(lang, "input-gamepad-left-thumb"),
        "RightThumb" => crate::t!(lang, "input-gamepad-right-thumb"),
        "LeftTrigger" => crate::t!(lang, "input-gamepad-left-shoulder"),
        "RightTrigger" => crate::t!(lang, "input-gamepad-right-shoulder"),
        "LeftTrigger2" => crate::t!(lang, "input-gamepad-axis-trigger-left"),
        "RightTrigger2" => crate::t!(lang, "input-gamepad-axis-trigger-right"),
        "DPadUp" => crate::t!(lang, "input-gamepad-dpad-up"),
        "DPadDown" => crate::t!(lang, "input-gamepad-dpad-down"),
        "DPadLeft" => crate::t!(lang, "input-gamepad-dpad-left"),
        "DPadRight" => crate::t!(lang, "input-gamepad-dpad-right"),
        other => other.to_string(),
    }
}

/// Localized display label for a stored gamepad-axis name. The
/// `+`/`−` direction sign is prepended by [`describe`]. `LeftZ` /
/// `RightZ` are the analog triggers on pads that report them as axes.
fn axis_label(lang: &unic_langid::LanguageIdentifier, name: &str) -> String {
    match name {
        "LeftStickX" => crate::t!(lang, "input-gamepad-axis-left-stick-x"),
        "LeftStickY" => crate::t!(lang, "input-gamepad-axis-left-stick-y"),
        "RightStickX" => crate::t!(lang, "input-gamepad-axis-right-stick-x"),
        "RightStickY" => crate::t!(lang, "input-gamepad-axis-right-stick-y"),
        "LeftZ" => crate::t!(lang, "input-gamepad-axis-trigger-left"),
        "RightZ" => crate::t!(lang, "input-gamepad-axis-trigger-right"),
        other => other.to_string(),
    }
}

/// What kind of physical source produced a binding — picks the chip's
/// glyph (keyboard vs gamepad) in the settings pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingKind {
    Keyboard,
    Gamepad,
}

/// Pretty-print a binding for the settings UI: the source kind (for
/// the chip's Lucide glyph) and a label. Gamepad names are localized;
/// keyboard names go through [`key_label`].
pub fn describe(lang: &unic_langid::LanguageIdentifier, b: &Binding) -> (BindingKind, String) {
    match b {
        Binding::Key(name) => (BindingKind::Keyboard, key_label(name)),
        Binding::Button(name) => (BindingKind::Gamepad, button_label(lang, name)),
        Binding::Axis { axis, dir } => {
            let sign = if *dir >= 0 { "+" } else { "−" };
            (BindingKind::Gamepad, format!("{sign}{}", axis_label(lang, axis)))
        }
    }
}

/// The GBA keys the user can rebind (plus speed-up). Drives the
/// settings pane's console layout and the per-key add/remove flow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

impl MappedKey {
    /// Canonical order — the Slint side indexes `input-lit` and the
    /// `input-key-selected(int)` callback by position in this array
    /// (ui/app.slint's input settings section), so the two must stay
    /// in sync.
    pub const ALL: [MappedKey; 11] = [
        MappedKey::Up,
        MappedKey::Down,
        MappedKey::Left,
        MappedKey::Right,
        MappedKey::A,
        MappedKey::B,
        MappedKey::L,
        MappedKey::R,
        MappedKey::Start,
        MappedKey::Select,
        MappedKey::SpeedUp,
    ];

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|&k| k == self).unwrap()
    }
}

/// Per-GBA-key binding lists. Each key can hold multiple bindings
/// (keyboard + gamepad simultaneously); any one of them being held
/// counts as the key being pressed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Mapping {
    pub up: Vec<Binding>,
    pub down: Vec<Binding>,
    pub left: Vec<Binding>,
    pub right: Vec<Binding>,
    pub a: Vec<Binding>,
    pub b: Vec<Binding>,
    pub l: Vec<Binding>,
    pub r: Vec<Binding>,
    pub start: Vec<Binding>,
    pub select: Vec<Binding>,
    pub speed_up: Vec<Binding>,
}

impl Default for Mapping {
    fn default() -> Self {
        // Keyboard: the historical fixed layout (Z=A, X=B, A=L, S=R,
        // Enter=Start, Space=Select, arrows, Shift=fast-forward).
        // Gamepad: tango's Xbox-layout defaults — d-pad + left stick
        // for directions, South/East for A/B, bumpers for L/R.
        let key = |s: &str| Binding::Key(s.to_string());
        let btn = |s: &str| Binding::Button(s.to_string());
        let axis = |s: &str, dir: i8| Binding::Axis {
            axis: s.to_string(),
            dir,
        };
        Self {
            up: vec![key("Up"), btn("DPadUp"), axis("LeftStickY", 1)],
            down: vec![key("Down"), btn("DPadDown"), axis("LeftStickY", -1)],
            left: vec![key("Left"), btn("DPadLeft"), axis("LeftStickX", -1)],
            right: vec![key("Right"), btn("DPadRight"), axis("LeftStickX", 1)],
            a: vec![key("Z"), btn("South")],
            b: vec![key("X"), btn("East")],
            l: vec![key("A"), btn("LeftTrigger")],
            r: vec![key("S"), btn("RightTrigger")],
            start: vec![key("Return"), btn("Start")],
            select: vec![key("Space"), btn("Select")],
            speed_up: vec![key("Shift")],
        }
    }
}

impl Mapping {
    /// The binding list for one mapped key — the settings pane's
    /// console view looks each key up through this.
    pub fn slot(&self, key: MappedKey) -> &Vec<Binding> {
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

    pub fn slot_mut(&mut self, key: MappedKey) -> &mut Vec<Binding> {
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

    /// Compute the mgba joyflag bitmask for the supplied held state.
    /// Speed-up isn't an mgba bit; check it via [`Self::speed_up_held`].
    pub fn to_joyflags(&self, held: &Held) -> u32 {
        use mgba::input::keys;
        let bit_if = |slot: &[Binding], bit: u32| -> u32 {
            if slot.iter().any(|b| held.is_active(b)) {
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

    pub fn speed_up_held(&self, held: &Held) -> bool {
        self.speed_up.iter().any(|b| held.is_active(b))
    }
}

/// Live held-input state combined from the keyboard (session /
/// settings FocusScope key events) and every connected gamepad (the
/// frame timer's gilrs poll).
#[derive(Default)]
pub struct Held {
    keys: HashSet<String>,
    buttons: HashSet<String>,
    /// Per-axis last-known normalized value in [-1, 1]. Bindings
    /// trigger when the value crosses [`AXIS_THRESHOLD`] in their
    /// direction.
    axes: HashMap<String, f32>,
}

impl Held {
    pub fn set_key(&mut self, name: &str, pressed: bool) {
        if pressed {
            self.keys.insert(name.to_string());
        } else {
            self.keys.remove(name);
        }
    }

    /// Whether `name` is currently held — lets edge-triggered actions
    /// (capture, replay pause toggle) tell a fresh press from OS
    /// key-repeat.
    pub fn is_key_held(&self, name: &str) -> bool {
        self.keys.contains(name)
    }

    /// Drop all keyboard state. Key releases are only delivered while
    /// a FocusScope is focused, so session start/end clears the set
    /// rather than trusting stale entries. Gamepad state is polled
    /// globally and never goes stale, so it survives.
    pub fn clear_keys(&mut self) {
        self.keys.clear();
    }

    pub fn set_button(&mut self, name: &str, pressed: bool) {
        if pressed {
            self.buttons.insert(name.to_string());
        } else {
            self.buttons.remove(name);
        }
    }

    pub fn set_axis(&mut self, name: &str, value: f32) {
        self.axes.insert(name.to_string(), value);
    }

    /// Last-known value for an axis (0.0 if never reported) — the
    /// capture flow uses it to bind on the threshold *crossing* so a
    /// stick resting off-center can't insta-bind.
    pub fn axis(&self, name: &str) -> f32 {
        self.axes.get(name).copied().unwrap_or(0.0)
    }

    /// Clear gamepad state — called on controller disconnect so
    /// stuck-pressed buttons don't leak across reconnects.
    pub fn clear_gamepad(&mut self) {
        self.buttons.clear();
        self.axes.clear();
    }

    pub fn is_active(&self, b: &Binding) -> bool {
        match b {
            Binding::Key(name) => self.keys.contains(name),
            Binding::Button(name) => self.buttons.contains(name),
            Binding::Axis { axis, dir } => {
                let v = self.axes.get(axis).copied().unwrap_or(0.0);
                if *dir >= 0 {
                    v > AXIS_THRESHOLD
                } else {
                    v < -AXIS_THRESHOLD
                }
            }
        }
    }
}
