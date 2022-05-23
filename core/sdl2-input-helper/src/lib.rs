pub struct ControllerState {
    buttons_pressed:
        [bool; sdl2::sys::SDL_GameControllerButton::SDL_CONTROLLER_BUTTON_MAX as usize],
    axes: [i16; sdl2::sys::SDL_GameControllerAxis::SDL_CONTROLLER_AXIS_MAX as usize],
}

impl ControllerState {
    pub fn new() -> Self {
        Self {
            buttons_pressed: [false;
                sdl2::sys::SDL_GameControllerButton::SDL_CONTROLLER_BUTTON_MAX as usize],
            axes: [0i16; sdl2::sys::SDL_GameControllerAxis::SDL_CONTROLLER_AXIS_MAX as usize],
        }
    }

    pub fn is_button_pressed(&self, button: sdl2::controller::Button) -> bool {
        self.buttons_pressed[button as usize]
    }

    pub fn axis(&self, axis: sdl2::controller::Axis) -> i16 {
        self.axes[axis as usize]
    }
}

pub struct State {
    keys_pressed: [bool; sdl2::keyboard::Scancode::Num as usize],
    controllers: std::collections::HashMap<u32, ControllerState>,
}

impl State {
    pub fn new() -> Self {
        Self {
            keys_pressed: [false; sdl2::keyboard::Scancode::Num as usize],
            controllers: std::collections::HashMap::new(),
        }
    }

    pub fn handle_event(&mut self, event: &sdl2::event::Event) -> bool {
        match event {
            sdl2::event::Event::KeyDown {
                scancode: Some(scancode),
                repeat: false,
                ..
            } => {
                self.keys_pressed[*scancode as usize] = true;
            }
            sdl2::event::Event::KeyUp {
                scancode: Some(scancode),
                repeat: false,
                ..
            } => {
                self.keys_pressed[*scancode as usize] = false;
            }
            sdl2::event::Event::ControllerDeviceAdded { which, .. } => {
                self.controllers.insert(*which, ControllerState::new());
            }
            sdl2::event::Event::ControllerDeviceRemoved { which, .. } => {
                self.controllers.remove(which);
            }
            sdl2::event::Event::ControllerAxisMotion {
                axis, value, which, ..
            } => {
                let controller = if let Some(controller) = self.controllers.get_mut(which) {
                    controller
                } else {
                    return false;
                };
                controller.axes[*axis as usize] = *value;
            }
            sdl2::event::Event::ControllerButtonDown { button, which, .. } => {
                let controller = if let Some(controller) = self.controllers.get_mut(which) {
                    controller
                } else {
                    return false;
                };
                controller.buttons_pressed[*button as usize] = true;
            }
            sdl2::event::Event::ControllerButtonUp { button, which, .. } => {
                let controller = if let Some(controller) = self.controllers.get_mut(which) {
                    controller
                } else {
                    return false;
                };
                controller.buttons_pressed[*button as usize] = false;
            }
            _ => {
                return false;
            }
        }
        true
    }

    pub fn is_key_pressed(&self, scancode: sdl2::keyboard::Scancode) -> bool {
        self.keys_pressed[scancode as usize]
    }

    pub fn iter_controllers(&self) -> impl std::iter::Iterator<Item = (&u32, &ControllerState)> {
        self.controllers.iter()
    }
}
