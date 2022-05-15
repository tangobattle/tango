#![windows_subsystem = "windows"]
use std::io::Write;

use clap::StructOpt;
use glium::Surface;

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
    egui_glium: egui_glium::EguiGlium,
    state: std::sync::Arc<GuiState>,
}

impl Gui {
    pub fn new(lang: String, display: &glium::Display) -> Self {
        let egui_glium = egui_glium::EguiGlium::new(display);

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

        egui_glium.egui_ctx.set_fonts(fonts);

        Self {
            egui_glium,
            state: std::sync::Arc::new(GuiState::new()),
        }
    }

    pub fn render(&mut self, display: &glium::Display, target: &mut impl glium::Surface) {
        self.egui_glium.run(&display, |ctx| {
            self.state.layout(ctx);
        });
        self.egui_glium.paint(&display, target);
    }

    pub fn state(&self) -> std::sync::Arc<GuiState> {
        self.state.clone()
    }
}

enum UserEvent {
    Gilrs(gilrs::Event),
}

#[derive(clap::Parser)]
struct Cli {
    #[clap(long)]
    lang: String,
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_default_env()
        .filter(Some("keymaptool"), log::LevelFilter::Info)
        .init();

    let args = Cli::parse();

    let event_loop = winit::event_loop::EventLoop::with_user_event();

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

    let cb = glium::glutin::ContextBuilder::new();

    let display = glium::Display::new(wb, cb, &event_loop)?;
    log::info!("GL version: {}", display.get_opengl_version_string());

    let mut gui = Gui::new(args.lang, &display);
    let gui_state = gui.state();

    let mut keys_pressed = [false; 255];

    let mut text = "".to_owned();
    match std::io::stdin().read_line(&mut text) {
        Ok(n) => {
            if n == 0 {
                return Ok(());
            }
        }
        Err(e) => {
            panic!("{}", e);
        }
    }
    gui_state.set_text(&text);

    let el_proxy = event_loop.create_proxy();
    let mut gilrs = gilrs::Gilrs::new().unwrap();
    std::thread::spawn(move || {
        while let Some(event) = gilrs.next_event() {
            if let Err(_) = el_proxy.send_event(UserEvent::Gilrs(event)) {
                break;
            }
        }
    });

    event_loop.run(move |event, _, control_flow| {
        match event {
            winit::event::Event::RedrawRequested(_) => {
                let mut target = display.draw();
                target.clear_color(0.0, 0.0, 0.0, 1.0);
                gui.render(&display, &mut target);
                target.finish().unwrap();
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
                    winit::event::WindowEvent::KeyboardInput { input, .. } => {
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
                                let mut text = "".to_owned();
                                match std::io::stdin().read_line(&mut text) {
                                    Ok(n) => {
                                        if n == 0 {
                                            *control_flow = winit::event_loop::ControlFlow::Exit;
                                            return;
                                        }
                                        display.gl_window().window().request_redraw();
                                    }
                                    Err(e) => {
                                        panic!("{}", e);
                                    }
                                }
                                gui_state.set_text(&text);
                            }
                            winit::event::ElementState::Released => {
                                keys_pressed[keycode as usize] = false;
                            }
                        }
                    }
                    _ => {}
                };
            }
            winit::event::Event::UserEvent(UserEvent::Gilrs(_gilrs_ev)) => {}
            _ => {}
        };
    });
}
