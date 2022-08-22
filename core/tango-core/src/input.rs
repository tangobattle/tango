pub struct StateTypes;
impl input_helper::StateTypes for StateTypes {
    type Key = glutin::event::VirtualKeyCode;
    type Button = sdl2::controller::Button;
}

pub type State = input_helper::State<StateTypes>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PhysicalInput {
    Key(glutin::event::VirtualKeyCode),
    Button(sdl2::controller::Button),
    Axis(sdl2::controller::Axis, AxisDirection),
}

#[derive(Clone, Debug, Copy, PartialEq, Eq)]
pub enum AxisDirection {
    Positive,
    Negative,
}

pub const AXIS_THRESHOLD: i16 = 0x4000;

impl PhysicalInput {
    pub fn is_active(&self, input: &State) -> bool {
        match *self {
            PhysicalInput::Key(key) => input.is_key_held(key),
            PhysicalInput::Button(button) => input
                .iter_controllers()
                .any(|(_, c)| c.is_button_held(button)),
            PhysicalInput::Axis(axis, dir) => input.iter_controllers().any(|(_, c)| {
                let v = c.axis(axis as usize);
                match dir {
                    AxisDirection::Positive => v > AXIS_THRESHOLD,
                    AxisDirection::Negative => v < -AXIS_THRESHOLD,
                }
            }),
        }
    }
}

#[derive(Clone, Debug)]
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
    pub speed_up: Vec<PhysicalInput>,
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
