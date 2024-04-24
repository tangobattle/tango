#[derive(Hash, Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum ControllerButton {
    #[serde(rename = "a")]
    A,
    #[serde(rename = "b")]
    B,
    #[serde(rename = "x")]
    X,
    #[serde(rename = "y")]
    Y,
    #[serde(rename = "back")]
    Back,
    #[serde(rename = "guide")]
    Guide,
    #[serde(rename = "start")]
    Start,
    #[serde(rename = "leftstick")]
    LeftStick,
    #[serde(rename = "rightstick")]
    RightStick,
    #[serde(rename = "leftshoulder")]
    LeftShoulder,
    #[serde(rename = "rightshoulder")]
    RightShoulder,
    #[serde(rename = "dpup")]
    DPadUp,
    #[serde(rename = "dpdown")]
    DPadDown,
    #[serde(rename = "dpleft")]
    DPadLeft,
    #[serde(rename = "dpright")]
    DPadRight,
    #[serde(rename = "misc1")]
    Misc1,
    #[serde(rename = "paddle1")]
    Paddle1,
    #[serde(rename = "paddle2")]
    Paddle2,
    #[serde(rename = "paddle3")]
    Paddle3,
    #[serde(rename = "paddle4")]
    Paddle4,
    #[serde(rename = "touchpad")]
    Touchpad,
}

impl ControllerButton {
    pub fn str(self) -> &'static str {
        match self {
            ControllerButton::A => "a",
            ControllerButton::B => "b",
            ControllerButton::X => "x",
            ControllerButton::Y => "y",
            ControllerButton::Back => "back",
            ControllerButton::Guide => "guide",
            ControllerButton::Start => "start",
            ControllerButton::LeftStick => "leftstick",
            ControllerButton::RightStick => "rightstick",
            ControllerButton::LeftShoulder => "leftshoulder",
            ControllerButton::RightShoulder => "rightshoulder",
            ControllerButton::DPadUp => "dpup",
            ControllerButton::DPadDown => "dpdown",
            ControllerButton::DPadLeft => "dpleft",
            ControllerButton::DPadRight => "dpright",
            ControllerButton::Misc1 => "misc1",
            ControllerButton::Paddle1 => "paddle1",
            ControllerButton::Paddle2 => "paddle2",
            ControllerButton::Paddle3 => "paddle3",
            ControllerButton::Paddle4 => "paddle4",
            ControllerButton::Touchpad => "touchpad",
        }
    }
}

#[cfg(not(target_os = "android"))]
impl From<sdl2::controller::Button> for ControllerButton {
    fn from(value: sdl2::controller::Button) -> Self {
        match value {
            sdl2::controller::Button::A => Self::A,
            sdl2::controller::Button::B => Self::B,
            sdl2::controller::Button::X => Self::X,
            sdl2::controller::Button::Y => Self::Y,
            sdl2::controller::Button::Back => Self::Back,
            sdl2::controller::Button::Guide => Self::Guide,
            sdl2::controller::Button::Start => Self::Start,
            sdl2::controller::Button::LeftStick => Self::LeftStick,
            sdl2::controller::Button::RightStick => Self::RightStick,
            sdl2::controller::Button::LeftShoulder => Self::LeftShoulder,
            sdl2::controller::Button::RightShoulder => Self::RightShoulder,
            sdl2::controller::Button::DPadUp => Self::DPadUp,
            sdl2::controller::Button::DPadDown => Self::DPadDown,
            sdl2::controller::Button::DPadLeft => Self::DPadLeft,
            sdl2::controller::Button::DPadRight => Self::DPadRight,
            sdl2::controller::Button::Misc1 => Self::Misc1,
            sdl2::controller::Button::Paddle1 => Self::Paddle1,
            sdl2::controller::Button::Paddle2 => Self::Paddle2,
            sdl2::controller::Button::Paddle3 => Self::Paddle3,
            sdl2::controller::Button::Paddle4 => Self::Paddle4,
            sdl2::controller::Button::Touchpad => Self::Touchpad,
        }
    }
}

#[derive(Hash, Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum ControllerAxis {
    #[serde(rename = "leftx")]
    LeftX,
    #[serde(rename = "lefty")]
    LeftY,
    #[serde(rename = "rightx")]
    RightX,
    #[serde(rename = "righty")]
    RightY,
    #[serde(rename = "lefttrigger")]
    TriggerLeft,
    #[serde(rename = "righttrigger")]
    TriggerRight,
}

impl ControllerAxis {
    pub fn str(self) -> &'static str {
        match self {
            Self::LeftX => "leftx",
            Self::LeftY => "lefty",
            Self::RightX => "rightx",
            Self::RightY => "righty",
            Self::TriggerLeft => "lefttrigger",
            Self::TriggerRight => "righttrigger",
        }
    }
}

#[cfg(not(target_os = "android"))]
impl From<sdl2::controller::Axis> for ControllerAxis {
    fn from(value: sdl2::controller::Axis) -> Self {
        match value {
            sdl2::controller::Axis::LeftX => Self::LeftX,
            sdl2::controller::Axis::LeftY => Self::LeftY,
            sdl2::controller::Axis::RightX => Self::RightX,
            sdl2::controller::Axis::RightY => Self::RightY,
            sdl2::controller::Axis::TriggerLeft => Self::TriggerLeft,
            sdl2::controller::Axis::TriggerRight => Self::TriggerRight,
        }
    }
}
