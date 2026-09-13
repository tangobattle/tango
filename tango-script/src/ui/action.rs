use serde::{Deserialize, Serialize};

/// Events are widget/interaction concepts. Domain actions stay in Luau.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Action {
    pub id: String,
    #[serde(flatten)]
    pub event: Event,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    Change {
        value: String,
    },
    Activate {
        value: String,
    },
    Toggle {
        checked: bool,
    },
    Slide {
        number: f64,
    },
    Scroll {
        x: f32,
        y: f32,
    },
    Pointer {
        phase: PointerPhase,
        x: f32,
        y: f32,
        button: PointerButton,
        dx: f32,
        dy: f32,
        modifiers: Modifiers,
    },
    Key {
        key: String,
        pressed: bool,
        modifiers: Modifiers,
    },
    Reorder {
        /// One-based positions in the script's children array.
        from: usize,
        to: usize,
    },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PointerPhase {
    Move,
    Down,
    Up,
    Leave,
    Wheel,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PointerButton {
    #[default]
    None,
    Left,
    Middle,
    Right,
    Other,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub command: bool,
}

impl Action {
    pub fn change(id: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            event: Event::Change { value: value.into() },
        }
    }
    pub fn activate(id: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            event: Event::Activate { value: value.into() },
        }
    }
    pub(crate) fn bounded(&self) -> bool {
        match &self.event {
            Event::Change { value } | Event::Activate { value } => value.len() <= 16 * 1024,
            Event::Slide { number } => number.is_finite(),
            Event::Scroll { x, y } => [x, y].into_iter().all(|n| n.is_finite() && (0.0..=1.0).contains(n)),
            Event::Pointer { x, y, dx, dy, .. } => {
                [x, y, dx, dy].iter().all(|n| n.is_finite() && n.abs() <= 1_000_000.0)
            }
            Event::Key { key, .. } => key.len() <= 128,
            _ => true,
        }
    }
}
