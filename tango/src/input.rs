use serde::Deserialize;

pub struct StateTypes;
impl input_helper::StateTypes for StateTypes {
    type Key = winit::event::VirtualKeyCode;
    type Button = sdl2::controller::Button;
}

pub type State = input_helper::State<StateTypes>;

fn serialize_sdl2_button<S>(v: &sdl2::controller::Button, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&v.string())
}

fn deserialize_sdl2_button<'de, D>(deserializer: D) -> Result<sdl2::controller::Button, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let buf = String::deserialize(deserializer)?;
    sdl2::controller::Button::from_string(&buf)
        .ok_or_else(|| serde::de::Error::invalid_value(serde::de::Unexpected::Str(&buf), &"valid sdl2 button"))
}

fn serialize_sdl2_axis<S>(v: &sdl2::controller::Axis, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&v.string())
}

fn deserialize_sdl2_axis<'de, D>(deserializer: D) -> Result<sdl2::controller::Axis, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let buf = String::deserialize(deserializer)?;
    sdl2::controller::Axis::from_string(&buf)
        .ok_or_else(|| serde::de::Error::invalid_value(serde::de::Unexpected::Str(&buf), &"valid sdl2 axis"))
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PhysicalInput {
    Key(winit::event::VirtualKeyCode),
    Button(
        #[serde(
            serialize_with = "serialize_sdl2_button",
            deserialize_with = "deserialize_sdl2_button"
        )]
        sdl2::controller::Button,
    ),
    Axis {
        #[serde(serialize_with = "serialize_sdl2_axis", deserialize_with = "deserialize_sdl2_axis")]
        axis: sdl2::controller::Axis,
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
                PhysicalInput::Key(winit::event::VirtualKeyCode::Up),
                PhysicalInput::Button(sdl2::controller::Button::DPadUp),
                PhysicalInput::Axis {
                    axis: sdl2::controller::Axis::LeftY,
                    direction: AxisDirection::Negative,
                },
            ],
            down: vec![
                PhysicalInput::Key(winit::event::VirtualKeyCode::Down),
                PhysicalInput::Button(sdl2::controller::Button::DPadDown),
                PhysicalInput::Axis {
                    axis: sdl2::controller::Axis::LeftY,
                    direction: AxisDirection::Positive,
                },
            ],
            left: vec![
                PhysicalInput::Key(winit::event::VirtualKeyCode::Left),
                PhysicalInput::Button(sdl2::controller::Button::DPadLeft),
                PhysicalInput::Axis {
                    axis: sdl2::controller::Axis::LeftX,
                    direction: AxisDirection::Negative,
                },
            ],
            right: vec![
                PhysicalInput::Key(winit::event::VirtualKeyCode::Right),
                PhysicalInput::Button(sdl2::controller::Button::DPadRight),
                PhysicalInput::Axis {
                    axis: sdl2::controller::Axis::LeftX,
                    direction: AxisDirection::Positive,
                },
            ],
            a: vec![
                PhysicalInput::Key(winit::event::VirtualKeyCode::Z),
                PhysicalInput::Button(sdl2::controller::Button::A),
            ],
            b: vec![
                PhysicalInput::Key(winit::event::VirtualKeyCode::X),
                PhysicalInput::Button(sdl2::controller::Button::B),
            ],
            l: vec![
                PhysicalInput::Key(winit::event::VirtualKeyCode::A),
                PhysicalInput::Button(sdl2::controller::Button::LeftShoulder),
                PhysicalInput::Axis {
                    axis: sdl2::controller::Axis::TriggerLeft,
                    direction: AxisDirection::Positive,
                },
            ],
            r: vec![
                PhysicalInput::Key(winit::event::VirtualKeyCode::S),
                PhysicalInput::Button(sdl2::controller::Button::RightShoulder),
                PhysicalInput::Axis {
                    axis: sdl2::controller::Axis::TriggerRight,
                    direction: AxisDirection::Positive,
                },
            ],
            select: vec![
                PhysicalInput::Key(winit::event::VirtualKeyCode::Space),
                PhysicalInput::Button(sdl2::controller::Button::Back),
            ],
            start: vec![
                PhysicalInput::Key(winit::event::VirtualKeyCode::Return),
                PhysicalInput::Button(sdl2::controller::Button::Start),
            ],
            speed_change: vec![PhysicalInput::Key(winit::event::VirtualKeyCode::LShift)],
            menu: vec![PhysicalInput::Key(winit::event::VirtualKeyCode::Escape)],
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
