use crate::controller::{ControllerAxis, ControllerButton};
use crate::keyboard::Key;
pub type State = input_helper::State<Key, ControllerButton>;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PhysicalInput {
    Key(Key),
    Button(ControllerButton),
    Axis {
        axis: ControllerAxis,
        direction: AxisDirection,
    },
}

#[derive(Clone, Debug, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AxisDirection {
    Positive,
    Negative,
}

pub const AXIS_THRESHOLD: i16 = 0x4000;

impl PhysicalInput {
    pub fn is_active(&self, input: &State) -> bool {
        match *self {
            PhysicalInput::Key(key) => input.is_key_held(key),
            PhysicalInput::Button(button) => input.iter_controllers().any(|(_, c)| c.is_button_held(button)),
            PhysicalInput::Axis { axis, direction } => input.iter_controllers().any(|(_, c)| {
                let v = c.axis(axis as usize);
                match direction {
                    AxisDirection::Positive => v > AXIS_THRESHOLD,
                    AxisDirection::Negative => v < -AXIS_THRESHOLD,
                }
            }),
        }
    }

    pub fn is_pressed(&self, input: &State) -> bool {
        match *self {
            PhysicalInput::Key(key) => input.is_key_pressed(key),
            PhysicalInput::Button(button) => input.iter_controllers().any(|(_, c)| c.is_button_pressed(button)),
            PhysicalInput::Axis { axis, direction } => input.iter_controllers().any(|(_, c)| {
                c.is_axis_leaving_threshold(
                    axis as usize,
                    match direction {
                        AxisDirection::Positive => AXIS_THRESHOLD,
                        AxisDirection::Negative => -AXIS_THRESHOLD,
                    },
                )
            }),
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
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
    pub select: Vec<PhysicalInput>,
    pub start: Vec<PhysicalInput>,
    pub speed_change: Vec<PhysicalInput>,
    pub menu: Vec<PhysicalInput>,
}

impl Default for Mapping {
    fn default() -> Self {
        Mapping {
            up: vec![
                PhysicalInput::Key(Key::Up),
                PhysicalInput::Button(ControllerButton::DPadUp),
                PhysicalInput::Axis {
                    axis: ControllerAxis::LeftY,
                    direction: AxisDirection::Negative,
                },
            ],
            down: vec![
                PhysicalInput::Key(Key::Down),
                PhysicalInput::Button(ControllerButton::DPadDown),
                PhysicalInput::Axis {
                    axis: ControllerAxis::LeftY,
                    direction: AxisDirection::Positive,
                },
            ],
            left: vec![
                PhysicalInput::Key(Key::Left),
                PhysicalInput::Button(ControllerButton::DPadLeft),
                PhysicalInput::Axis {
                    axis: ControllerAxis::LeftX,
                    direction: AxisDirection::Negative,
                },
            ],
            right: vec![
                PhysicalInput::Key(Key::Right),
                PhysicalInput::Button(ControllerButton::DPadRight),
                PhysicalInput::Axis {
                    axis: ControllerAxis::LeftX,
                    direction: AxisDirection::Positive,
                },
            ],
            a: vec![PhysicalInput::Key(Key::Z), PhysicalInput::Button(ControllerButton::A)],
            b: vec![PhysicalInput::Key(Key::X), PhysicalInput::Button(ControllerButton::B)],
            l: vec![
                PhysicalInput::Key(Key::A),
                PhysicalInput::Button(ControllerButton::LeftShoulder),
                PhysicalInput::Axis {
                    axis: ControllerAxis::TriggerLeft,
                    direction: AxisDirection::Positive,
                },
            ],
            r: vec![
                PhysicalInput::Key(Key::S),
                PhysicalInput::Button(ControllerButton::RightShoulder),
                PhysicalInput::Axis {
                    axis: ControllerAxis::TriggerRight,
                    direction: AxisDirection::Positive,
                },
            ],
            select: vec![
                PhysicalInput::Key(Key::Space),
                PhysicalInput::Button(ControllerButton::Back),
            ],
            start: vec![
                PhysicalInput::Key(Key::Return),
                PhysicalInput::Button(ControllerButton::Start),
            ],
            speed_change: vec![PhysicalInput::Key(Key::LShift)],
            menu: vec![PhysicalInput::Key(Key::Escape)],
        }
    }
}

impl Mapping {
    pub fn to_mgba_keys(&self, input: &State) -> u32 {
        (if self.left.iter().any(|c| c.is_active(input)) {
            mgba::input::keys::LEFT
        } else {
            0
        }) | (if self.right.iter().any(|c| c.is_active(input)) {
            mgba::input::keys::RIGHT
        } else {
            0
        }) | (if self.up.iter().any(|c| c.is_active(input)) {
            mgba::input::keys::UP
        } else {
            0
        }) | (if self.down.iter().any(|c| c.is_active(input)) {
            mgba::input::keys::DOWN
        } else {
            0
        }) | (if self.a.iter().any(|c| c.is_active(input)) {
            mgba::input::keys::A
        } else {
            0
        }) | (if self.b.iter().any(|c| c.is_active(input)) {
            mgba::input::keys::B
        } else {
            0
        }) | (if self.l.iter().any(|c| c.is_active(input)) {
            mgba::input::keys::L
        } else {
            0
        }) | (if self.r.iter().any(|c| c.is_active(input)) {
            mgba::input::keys::R
        } else {
            0
        }) | (if self.select.iter().any(|c| c.is_active(input)) {
            mgba::input::keys::SELECT
        } else {
            0
        }) | (if self.start.iter().any(|c| c.is_active(input)) {
            mgba::input::keys::START
        } else {
            0
        })
    }
}
