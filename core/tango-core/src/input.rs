#[derive(Clone, Debug)]
pub struct Input {
    pub local_tick: u32,
    pub remote_tick: u32,
    pub joyflags: u16,
    pub custom_screen_state: u8,
    pub turn: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct PartialInput {
    pub local_tick: u32,
    pub remote_tick: u32,
    pub joyflags: u16,
}

impl Input {
    pub fn lag(&self) -> i32 {
        self.remote_tick as i32 - self.local_tick as i32
    }
}

pub struct PairQueue<T, U>
where
    T: Clone,
    U: Clone,
{
    local_queue: std::collections::VecDeque<T>,
    remote_queue: std::collections::VecDeque<U>,
    local_delay: u32,
}

#[derive(Clone, Debug)]
pub struct Pair<T, U>
where
    T: Clone,
    U: Clone,
{
    pub local: T,
    pub remote: U,
}

impl<T, U> PairQueue<T, U>
where
    T: Clone,
    U: Clone,
{
    pub fn new(capacity: usize, local_delay: u32) -> Self {
        PairQueue {
            local_queue: std::collections::VecDeque::with_capacity(capacity),
            remote_queue: std::collections::VecDeque::with_capacity(capacity),
            local_delay,
        }
    }

    pub fn add_local_input(&mut self, v: T) {
        self.local_queue.push_back(v);
    }

    pub fn add_remote_input(&mut self, v: U) {
        self.remote_queue.push_back(v);
    }

    pub fn local_delay(&self) -> u32 {
        self.local_delay
    }

    pub fn local_queue_length(&self) -> usize {
        self.local_queue.len()
    }

    pub fn remote_queue_length(&self) -> usize {
        self.remote_queue.len()
    }

    pub fn consume_and_peek_local(&mut self) -> (Vec<Pair<T, U>>, Vec<T>) {
        let to_commit = {
            let mut n = self.local_queue.len() as isize - self.local_delay as isize;
            if (self.remote_queue.len() as isize) < n {
                n = self.remote_queue.len() as isize;
            }

            if n < 0 {
                vec![]
            } else {
                let local_inputs = self.local_queue.drain(..n as usize);
                let remote_inputs = self.remote_queue.drain(..n as usize);
                local_inputs
                    .zip(remote_inputs)
                    .map(|(local, remote)| Pair { local, remote })
                    .collect()
            }
        };

        let peeked = {
            let n = self.local_queue.len() as isize - self.local_delay as isize;
            if n < 0 {
                vec![]
            } else {
                self.local_queue.range(..n as usize).cloned().collect()
            }
        };

        (to_commit, peeked)
    }
}

pub struct InputState {
    keys_pressed: [bool; sdl2::keyboard::Scancode::Num as usize],
    buttons_pressed:
        [bool; sdl2::sys::SDL_GameControllerButton::SDL_CONTROLLER_BUTTON_MAX as usize],
    axes: [i16; sdl2::sys::SDL_GameControllerAxis::SDL_CONTROLLER_AXIS_MAX as usize],
}

impl InputState {
    pub fn new() -> Self {
        Self {
            keys_pressed: [false; sdl2::keyboard::Scancode::Num as usize],
            buttons_pressed: [false;
                sdl2::sys::SDL_GameControllerButton::SDL_CONTROLLER_BUTTON_MAX as usize],
            axes: [0i16; sdl2::sys::SDL_GameControllerAxis::SDL_CONTROLLER_AXIS_MAX as usize],
        }
    }

    pub fn handle_event(&mut self, event: sdl2::event::Event) -> bool {
        match event {
            sdl2::event::Event::KeyDown {
                scancode: Some(scancode),
                repeat: false,
                ..
            } => {
                self.keys_pressed[scancode as usize] = true;
            }
            sdl2::event::Event::KeyUp {
                scancode: Some(scancode),
                repeat: false,
                ..
            } => {
                self.keys_pressed[scancode as usize] = false;
            }
            sdl2::event::Event::ControllerAxisMotion { axis, value, .. } => {
                self.axes[axis as usize] = value;
            }
            sdl2::event::Event::ControllerButtonDown { button, .. } => {
                self.buttons_pressed[button as usize] = true;
            }
            sdl2::event::Event::ControllerButtonUp { button, .. } => {
                self.buttons_pressed[button as usize] = false;
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

    pub fn is_button_pressed(&self, button: sdl2::controller::Button) -> bool {
        self.buttons_pressed[button as usize]
    }

    pub fn axis(&self, axis: sdl2::controller::Axis) -> i16 {
        self.axes[axis as usize]
    }
}
