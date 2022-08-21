pub struct ControllerState {
    buttons_pressed: std::collections::HashSet<usize>,
    axes: Vec<i16>,
}

impl ControllerState {
    pub fn new(num_axes: usize) -> Self {
        Self {
            buttons_pressed: std::collections::HashSet::new(),
            axes: vec![0; num_axes],
        }
    }

    pub fn is_button_pressed(&self, button: usize) -> bool {
        self.buttons_pressed.contains(&button)
    }

    pub fn axis(&self, axis: usize) -> i16 {
        self.axes[axis]
    }
}

pub struct State {
    keys_pressed: std::collections::HashSet<usize>,
    controllers: std::collections::HashMap<u32, ControllerState>,
}

impl State {
    pub fn new() -> Self {
        Self {
            keys_pressed: std::collections::HashSet::new(),
            controllers: std::collections::HashMap::new(),
        }
    }

    pub fn handle_key_up(&mut self, key: usize) {
        self.keys_pressed.remove(&key);
    }

    pub fn handle_key_down(&mut self, key: usize) {
        self.keys_pressed.insert(key);
    }

    pub fn handle_controller_axis_motion(&mut self, id: u32, axis: usize, value: i16) {
        let controller_state = if let Some(controller_state) = self.controllers.get_mut(&id) {
            controller_state
        } else {
            return;
        };
        controller_state.axes[axis] = value;
    }

    pub fn handle_controller_button_up(&mut self, id: u32, button: usize) {
        let controller_state = if let Some(controller_state) = self.controllers.get_mut(&id) {
            controller_state
        } else {
            return;
        };
        controller_state.buttons_pressed.remove(&button);
    }

    pub fn handle_controller_button_down(&mut self, id: u32, button: usize) {
        let controller_state = if let Some(controller_state) = self.controllers.get_mut(&id) {
            controller_state
        } else {
            return;
        };
        controller_state.buttons_pressed.insert(button);
    }

    pub fn handle_controller_connected(&mut self, id: u32, num_axes: usize) {
        self.controllers.insert(id, ControllerState::new(num_axes));
    }

    pub fn handle_controller_disconnected(&mut self, id: u32) {
        self.controllers.remove(&id);
    }

    pub fn is_key_pressed(&self, scancode: usize) -> bool {
        self.keys_pressed.contains(&scancode)
    }

    pub fn iter_controllers(&self) -> impl std::iter::Iterator<Item = (&u32, &ControllerState)> {
        self.controllers.iter()
    }
}
