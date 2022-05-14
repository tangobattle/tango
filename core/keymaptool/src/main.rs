#![windows_subsystem = "windows"]
use std::io::Write;

use clap::StructOpt;

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
        egui::panel::CentralPanel::default().show(ctx, |ui| {
            ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(self.text.lock().clone()).size(32.0));
            });
        });
    }
}

struct Gui {
    ctx: egui::Context,
    winit_state: egui_winit::State,
    screen_descriptor: egui_wgpu_backend::ScreenDescriptor,
    rpass: egui_wgpu_backend::RenderPass,
    paint_jobs: Vec<egui::ClippedMesh>,
    textures: egui::TexturesDelta,
    state: std::sync::Arc<GuiState>,
}

impl Gui {
    pub fn new(
        lang: String,
        width: u32,
        height: u32,
        scale_factor: f32,
        pixels: &pixels::Pixels,
    ) -> Self {
        let max_texture_size = pixels.device().limits().max_texture_dimension_2d as usize;

        let ctx = egui::Context::default();

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
        ctx.set_fonts(fonts);

        ctx.set_visuals(egui::Visuals::light());

        let winit_state = egui_winit::State::from_pixels_per_point(max_texture_size, scale_factor);
        let screen_descriptor = egui_wgpu_backend::ScreenDescriptor {
            physical_width: width,
            physical_height: height,
            scale_factor,
        };
        let rpass =
            egui_wgpu_backend::RenderPass::new(pixels.device(), pixels.render_texture_format(), 1);
        let textures = egui::TexturesDelta::default();

        Self {
            ctx,
            winit_state,
            screen_descriptor,
            rpass,
            paint_jobs: Vec::new(),
            textures,
            state: std::sync::Arc::new(GuiState::new()),
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.screen_descriptor.physical_width = width;
            self.screen_descriptor.physical_height = height;
        }
    }

    pub fn prepare(&mut self, window: &winit::window::Window) {
        let raw_input = self.winit_state.take_egui_input(window);
        let output = self.ctx.run(raw_input, |ctx| {
            self.state.layout(ctx);
        });

        self.textures.append(output.textures_delta);
        self.winit_state
            .handle_platform_output(window, &self.ctx, output.platform_output);
        self.paint_jobs = self.ctx.tessellate(output.shapes);
    }

    pub fn render(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        render_target: &wgpu::TextureView,
        context: &pixels::PixelsContext,
    ) -> Result<(), egui_wgpu_backend::BackendError> {
        self.rpass
            .add_textures(&context.device, &context.queue, &self.textures)?;
        self.rpass.update_buffers(
            &context.device,
            &context.queue,
            &self.paint_jobs,
            &self.screen_descriptor,
        );

        self.rpass.execute(
            encoder,
            render_target,
            &self.paint_jobs,
            &self.screen_descriptor,
            None,
        )?;

        let textures = std::mem::take(&mut self.textures);
        self.rpass.remove_textures(textures)
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

    let event_loop = Some(winit::event_loop::EventLoop::with_user_event());

    let size = winit::dpi::LogicalSize::new(400u32, 100u32);
    let window = winit::window::WindowBuilder::new()
        .with_title("keymaptool")
        .with_inner_size(size)
        .with_min_inner_size(size)
        .with_always_on_top(true)
        .with_decorations(false)
        .build(event_loop.as_ref().expect("event loop"))?;

    let window_size = window.inner_size();
    let surface_texture =
        pixels::SurfaceTexture::new(window_size.width, window_size.height, &window);
    let mut pixels =
        pixels::PixelsBuilder::new(size.width, size.height, surface_texture).build()?;
    let mut gui = Gui::new(
        args.lang,
        window_size.width,
        window_size.height,
        window.scale_factor() as f32,
        &pixels,
    );
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

    let el_proxy = event_loop.as_ref().expect("event loop").create_proxy();
    let mut gilrs = gilrs::Gilrs::new().unwrap();
    std::thread::spawn(move || {
        while let Some(event) = gilrs.next_event() {
            if let Err(_) = el_proxy.send_event(UserEvent::Gilrs(event)) {
                break;
            }
        }
    });

    event_loop
        .expect("event loop")
        .run(move |event, _, control_flow| {
            match event {
                winit::event::Event::RedrawRequested(_) => {
                    gui.prepare(&window);
                    pixels
                        .render_with(|encoder, render_target, context| {
                            context.scaling_renderer.render(encoder, render_target);
                            gui.render(encoder, render_target, context)?;
                            Ok(())
                        })
                        .expect("render pixels");
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
                                        .write_all(
                                            serde_plain::to_string(&keycode).unwrap().as_bytes(),
                                        )
                                        .unwrap();
                                    std::io::stdout().write_all(b"\n").unwrap();
                                    let mut text = "".to_owned();
                                    match std::io::stdin().read_line(&mut text) {
                                        Ok(n) => {
                                            if n == 0 {
                                                *control_flow =
                                                    winit::event_loop::ControlFlow::Exit;
                                                return;
                                            }
                                            window.request_redraw();
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
                        winit::event::WindowEvent::Resized(size) => {
                            pixels.resize_surface(size.width, size.height);
                            gui.resize(size.width, size.height);
                        }
                        _ => {}
                    };
                }
                winit::event::Event::UserEvent(UserEvent::Gilrs(_gilrs_ev)) => {}
                _ => {}
            };
        });
}
