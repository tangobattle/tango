#![windows_subsystem = "windows"]
use std::io::Write;

use clap::StructOpt;
use glow::HasContext;

struct GuiState {
    text: parking_lot::Mutex<String>,
}

impl GuiState {
    fn new() -> Self {
        Self {
            text: parking_lot::Mutex::new("".to_owned()),
        }
    }

    fn set_text(&self, text: &str) {
        *self.text.lock() = text.to_owned();
    }

    fn layout(&self, ctx: &egui::Context) {
        ctx.set_visuals(egui::Visuals::light());
        egui::panel::CentralPanel::default().show(ctx, |ui| {
            ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(self.text.lock().clone()).size(32.0));
            });
        });
    }
}

struct Gui {
    egui_glow: egui_glow::EguiGlow,
    state: std::sync::Arc<GuiState>,
}

impl Gui {
    pub fn new(lang: String, window: &winit::window::Window, gl: &glow::Context) -> Self {
        let egui_glow = egui_glow::EguiGlow::new(window, gl);

        let mut fonts = egui::FontDefinitions::default();
        let font = match lang.as_str() {
            "ja" => egui::FontData::from_static(include_bytes!("fonts/NotoSansJP-Regular.otf")),
            "zh-Hans" => {
                egui::FontData::from_static(include_bytes!("fonts/NotoSansSC-Regular.otf"))
            }
            _ => egui::FontData::from_static(include_bytes!("fonts/NotoSans-Regular.ttf")),
        };
        fonts.font_data.insert("main".to_owned(), font);
        *fonts
            .families
            .get_mut(&egui::FontFamily::Proportional)
            .unwrap() = vec!["main".to_owned()];

        egui_glow.egui_ctx.set_fonts(fonts);

        Self {
            egui_glow,
            state: std::sync::Arc::new(GuiState::new()),
        }
    }

    pub fn render(&mut self, window: &winit::window::Window, gl: &glow::Context) {
        self.egui_glow.run(window, |ctx| {
            self.state.layout(ctx);
        });
        self.egui_glow.paint(window, gl);
    }

    pub fn state(&self) -> std::sync::Arc<GuiState> {
        self.state.clone()
    }
}

#[derive(clap::ArgEnum, Clone, PartialEq)]
enum Target {
    Keyboard,
    Controller,
}

#[derive(clap::Parser)]
struct Cli {
    #[clap(long)]
    lang: String,

    #[clap(arg_enum, long)]
    target: Target,
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_default_env()
        .filter(Some("keymaptool"), log::LevelFilter::Info)
        .init();

    let args = Cli::parse();

    let event_loop = winit::event_loop::EventLoop::new();

    let handle = event_loop.primary_monitor().unwrap();
    let monitor_size = handle.size();
    let size = winit::dpi::LogicalSize::new(400u32, 100u32);
    let wb = winit::window::WindowBuilder::new()
        .with_title("keymaptool")
        .with_position(winit::dpi::PhysicalPosition {
            x: monitor_size.width / 2 - 400 / 2,
            y: monitor_size.height / 2 - 100 / 2,
        })
        .with_inner_size(size)
        .with_min_inner_size(size)
        .with_always_on_top(true)
        .with_decorations(false);

    let gl_window = unsafe {
        glutin::ContextBuilder::new()
            .with_double_buffer(Some(true))
            .with_vsync(true)
            .build_windowed(wb, &event_loop)
            .unwrap()
            .make_current()
            .unwrap()
    };

    let gl = unsafe { glow::Context::from_loader_function(|s| gl_window.get_proc_address(s)) };
    log::info!("GL version: {}", unsafe {
        gl.get_parameter_string(glow::VERSION)
    });

    let mut gui = Gui::new(args.lang, gl_window.window(), &gl);
    let gui_state = gui.state();

    let next = move || {
        let mut text = "".to_owned();
        match std::io::stdin().read_line(&mut text) {
            Ok(n) => {
                if n == 0 {
                    return false;
                }
            }
            Err(e) => {
                panic!("{}", e);
            }
        }
        gui_state.set_text(&text);
        true
    };
    if !next() {
        return Ok(());
    }

    let mut gilrs = gilrs::Gilrs::new().unwrap();

    let mut keys_pressed = [false; 255];
    let mut buttons_pressed = [false; 255];

    event_loop.run(move |event, _, control_flow| {
        match event {
            winit::event::Event::RedrawRequested(_) => {
                unsafe {
                    gl.clear_color(0.0, 0.0, 0.0, 1.0);
                    gl.clear(glow::COLOR_BUFFER_BIT);
                }
                gui.render(gl_window.window(), &gl);
                gl_window.swap_buffers().unwrap();
            }
            winit::event::Event::MainEventsCleared => {
                if args.target == Target::Controller {
                    while let Some(gilrs::Event { event, .. }) = gilrs.next_event() {
                        let (button, pressed) = match event {
                            gilrs::EventType::ButtonPressed(button, _) => (button, true),
                            gilrs::EventType::ButtonRepeated(button, _) => (button, false),
                            _ => continue,
                        };

                        if pressed {
                            if buttons_pressed[button as usize] {
                                continue;
                            }
                            buttons_pressed[button as usize] = true;

                            std::io::stdout()
                                .write_all(serde_plain::to_string(&button).unwrap().as_bytes())
                                .unwrap();
                            std::io::stdout().write_all(b"\n").unwrap();
                            if !next() {
                                *control_flow = winit::event_loop::ControlFlow::Exit;
                                return;
                            }
                            gl_window.window().request_redraw();
                        } else {
                            buttons_pressed[button as usize] = false;
                        }
                    }
                }
            }
            winit::event::Event::WindowEvent {
                event: ref window_event,
                ..
            } => {
                match window_event {
                    winit::event::WindowEvent::CloseRequested
                    | winit::event::WindowEvent::Focused(false) => {
                        *control_flow = winit::event_loop::ControlFlow::Exit;
                    }
                    winit::event::WindowEvent::KeyboardInput { input, .. }
                        if args.target == Target::Keyboard =>
                    {
                        let keycode = if let Some(keycode) = input.virtual_keycode {
                            keycode
                        } else {
                            return;
                        };
                        match input.state {
                            winit::event::ElementState::Pressed => {
                                if keys_pressed[keycode as usize] {
                                    return;
                                }
                                keys_pressed[keycode as usize] = true;

                                std::io::stdout()
                                    .write_all(serde_plain::to_string(&keycode).unwrap().as_bytes())
                                    .unwrap();
                                std::io::stdout().write_all(b"\n").unwrap();
                                if !next() {
                                    *control_flow = winit::event_loop::ControlFlow::Exit;
                                    return;
                                }
                                gl_window.window().request_redraw();
                            }
                            winit::event::ElementState::Released => {
                                keys_pressed[keycode as usize] = false;
                            }
                        }
                    }
                    _ => {}
                };
            }
            _ => {}
        };
    });
}
