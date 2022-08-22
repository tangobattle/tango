#[derive(Clone)]
pub struct ControllerState {
    buttons_held: std::collections::HashSet<usize>,
    last_buttons_held: std::collections::HashSet<usize>,
    axes: Vec<i16>,
    last_axes: Vec<i16>,
}

impl ControllerState {
    pub fn new(num_axes: usize) -> Self {
        Self {
            buttons_held: std::collections::HashSet::new(),
            last_buttons_held: std::collections::HashSet::new(),
            axes: vec![0; num_axes],
            last_axes: vec![0; num_axes],
        }
    }

    pub fn is_button_pressed(&self, button: usize) -> bool {
        !self.last_buttons_held.contains(&button) && self.buttons_held.contains(&button)
    }

    pub fn is_button_released(&self, button: usize) -> bool {
        self.last_buttons_held.contains(&button) && !self.buttons_held.contains(&button)
    }

    pub fn is_button_held(&self, button: usize) -> bool {
        self.buttons_held.contains(&button)
    }

    pub fn axis(&self, axis: usize) -> i16 {
        self.axes[axis]
    }

    pub fn axis_delta(&self, axis: usize) -> i16 {
        self.axes[axis] - self.last_axes[axis]
    }

    pub fn digest(&mut self) {
        self.last_buttons_held = self.buttons_held.clone();
        self.last_axes = self.axes.clone();
    }
}

#[derive(Clone)]
pub struct State {
    keys_held: std::collections::HashSet<usize>,
    last_keys_held: std::collections::HashSet<usize>,
    controllers: std::collections::HashMap<u32, ControllerState>,
}

impl State {
    pub fn new() -> Self {
        Self {
            last_keys_held: std::collections::HashSet::new(),
            keys_held: std::collections::HashSet::new(),
            controllers: std::collections::HashMap::new(),
        }
    }

    pub fn handle_key_up(&mut self, key: usize) {
        self.keys_held.remove(&key);
    }

    pub fn handle_key_down(&mut self, key: usize) {
        self.keys_held.insert(key);
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
        controller_state.buttons_held.remove(&button);
    }

    pub fn handle_controller_button_down(&mut self, id: u32, button: usize) {
        let controller_state = if let Some(controller_state) = self.controllers.get_mut(&id) {
            controller_state
        } else {
            return;
        };
        controller_state.buttons_held.insert(button);
    }

    pub fn handle_controller_connected(&mut self, id: u32, num_axes: usize) {
        self.controllers.insert(id, ControllerState::new(num_axes));
    }

    pub fn handle_controller_disconnected(&mut self, id: u32) {
        self.controllers.remove(&id);
    }

    pub fn is_key_pressed(&self, scancode: usize) -> bool {
        !self.last_keys_held.contains(&scancode) && self.keys_held.contains(&scancode)
    }

    pub fn is_key_released(&self, scancode: usize) -> bool {
        self.last_keys_held.contains(&scancode) && !self.keys_held.contains(&scancode)
    }

    pub fn is_key_held(&self, scancode: usize) -> bool {
        self.keys_held.contains(&scancode)
    }

    pub fn iter_controllers(&self) -> impl std::iter::Iterator<Item = (&u32, &ControllerState)> {
        self.controllers.iter()
    }

    pub fn digest(&mut self) {
        self.last_keys_held = self.keys_held.clone();

        for (_, controller) in self.controllers.iter_mut() {
            controller.digest();
        }
    }
}
