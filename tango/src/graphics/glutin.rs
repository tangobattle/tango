use glow::HasContext;

use crate::graphics;

pub struct Backend {
    gl_window: glutin::ContextWrapper<glutin::PossiblyCurrent, winit::window::Window>,
    gl: std::sync::Arc<glow::Context>,
    egui_glow: egui_glow::EguiGlow,
    ui_scale: f32,
}

impl Backend {
    pub fn new<C: glutin::ContextCurrentState, T>(
        gl_window: glutin::ContextWrapper<C, winit::window::Window>,
        event_loop: &winit::event_loop::EventLoopWindowTarget<T>,
    ) -> Self {
        let gl_window = unsafe { gl_window.make_current().unwrap() };
        let gl = std::sync::Arc::new(unsafe { glow::Context::from_loader_function(|s| gl_window.get_proc_address(s)) });

        let mut egui_glow = egui_glow::EguiGlow::new(&event_loop, gl.clone());
        egui_glow
            .egui_winit
            .set_pixels_per_point(gl_window.window().scale_factor() as f32);

        log::info!(
            "GL version: {}, extensions: {:?}",
            unsafe { gl.get_parameter_string(glow::VERSION) },
            gl.supported_extensions()
        );

        Self {
            gl_window,
            gl: gl.clone(),
            egui_glow,
            ui_scale: 1.0,
        }
    }
}

impl graphics::Backend for Backend {
    fn set_ui_scale(&mut self, scale: f32) {
        self.ui_scale = scale;
        self.egui_glow
            .egui_winit
            .set_pixels_per_point(self.gl_window.window().scale_factor() as f32 * self.ui_scale);
    }

    fn window(&self) -> &winit::window::Window {
        self.gl_window.window()
    }

    fn paint(&mut self) {
        unsafe {
            self.gl.clear_color(0.0, 0.0, 0.0, 1.0);
            self.gl.clear(glow::COLOR_BUFFER_BIT);
        }
        self.egui_glow.paint(self.gl_window.window());
        self.gl_window.swap_buffers().unwrap()
    }

    fn egui_ctx(&self) -> &egui::Context {
        &self.egui_glow.egui_ctx
    }

    fn run<'a>(
        &mut self,
        mut run_ui: Box<dyn FnMut(&winit::window::Window, &egui::Context) + 'a>,
    ) -> std::time::Duration {
        let window = self.gl_window.window();
        self.egui_glow.run(window, |ui| run_ui(window, ui))
    }

    fn on_window_event(&mut self, event: &winit::event::WindowEvent) -> bool {
        match event {
            winit::event::WindowEvent::Resized(physical_size) => {
                if physical_size.width > 0 && physical_size.height > 0 {
                    self.gl_window.resize(*physical_size);
                }
            }
            winit::event::WindowEvent::ScaleFactorChanged { new_inner_size, .. } => {
                self.egui_glow
                    .egui_winit
                    .set_pixels_per_point(self.gl_window.window().scale_factor() as f32 * self.ui_scale);
                self.gl_window.resize(**new_inner_size);
            }
            _ => {}
        }
        self.egui_glow.on_event(event)
    }
}
