// This code is from https://github.com/rukai/winit_input_helper, with modifications.
//
// The MIT License (MIT)
//
// Copyright (c) 2018 Lucas Kent
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:

// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.

// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

use winit::event::{ElementState, MouseButton, MouseScrollDelta, VirtualKeyCode, WindowEvent};

#[derive(Clone)]
pub struct CurrentInput {
    pub mouse_actions: Vec<MouseAction>,
    pub key_actions: Vec<KeyAction>,
    pub key_held: [bool; 255],
    pub mouse_held: [bool; 255],
    pub mouse_point: Option<(f32, f32)>,
    pub mouse_point_prev: Option<(f32, f32)>,
    pub scroll_diff: f32,
}

impl CurrentInput {
    pub fn new() -> CurrentInput {
        CurrentInput {
            mouse_actions: vec![],
            key_actions: vec![],
            key_held: [false; 255],
            mouse_held: [false; 255],
            mouse_point: None,
            mouse_point_prev: None,
            scroll_diff: 0.0,
        }
    }

    pub fn step(&mut self) {
        self.mouse_actions.clear();
        self.key_actions.clear();
        self.scroll_diff = 0.0;
        self.mouse_point_prev = self.mouse_point;
    }

    pub fn handle_event(&mut self, event: &WindowEvent) {
        match event {
            WindowEvent::KeyboardInput { input, .. } => {
                if let Some(keycode) = input.virtual_keycode {
                    match input.state {
                        ElementState::Pressed => {
                            if !self.key_held[keycode as usize] {
                                self.key_actions.push(KeyAction::Pressed(keycode));
                            }
                            self.key_held[keycode as usize] = true;
                            self.key_actions.push(KeyAction::PressedOs(keycode));
                        }
                        ElementState::Released => {
                            self.key_held[keycode as usize] = false;
                            self.key_actions.push(KeyAction::Released(keycode));
                        }
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_point = Some((position.x as f32, position.y as f32));
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button,
                ..
            } => {
                let button = mouse_button_to_int(button);
                self.mouse_held[button] = true;
                self.mouse_actions.push(MouseAction::Pressed(button));
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button,
                ..
            } => {
                let button = mouse_button_to_int(button);
                self.mouse_held[button] = false;
                self.mouse_actions.push(MouseAction::Released(button));
            }
            WindowEvent::MouseWheel { delta, .. } => {
                // I just took this from three-rs, no idea why this magic number was chosen ¯\_(ツ)_/¯
                const PIXELS_PER_LINE: f64 = 38.0;

                match delta {
                    MouseScrollDelta::LineDelta(_, y) => {
                        self.scroll_diff += y;
                    }
                    MouseScrollDelta::PixelDelta(delta) => {
                        self.scroll_diff += (delta.y / PIXELS_PER_LINE) as f32
                    }
                }
            }
            _ => {}
        }
    }
}

#[derive(Clone)]
pub enum KeyAction {
    Pressed(VirtualKeyCode),
    PressedOs(VirtualKeyCode),
    Released(VirtualKeyCode),
}

#[derive(Clone)]
pub enum MouseAction {
    Pressed(usize),
    Released(usize),
}

fn mouse_button_to_int(button: &MouseButton) -> usize {
    match button {
        MouseButton::Left => 0,
        MouseButton::Right => 1,
        MouseButton::Middle => 2,
        MouseButton::Other(byte) => *byte as usize,
    }
}
