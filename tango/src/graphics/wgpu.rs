use crate::graphics;

pub struct Backend<'a> {
    window: winit::window::Window,
    egui_ctx: egui::Context,
    painter: egui_wgpu::winit::Painter<'a>,
    egui_winit: egui_winit::State,
    shapes: Vec<egui::epaint::ClippedShape>,
    textures_delta: egui::TexturesDelta,
}

impl<'a> Backend<'a> {
    pub fn new<T>(
        window: winit::window::Window,
        mut painter: egui_wgpu::winit::Painter<'a>,
        event_loop: &winit::event_loop::EventLoopWindowTarget<T>,
    ) -> Self {
        unsafe {
            painter.set_window(Some(&window));
        }
        let mut egui_winit = egui_winit::State::new(event_loop);
        egui_winit.set_pixels_per_point(window.scale_factor() as f32);
        egui_winit.set_max_texture_side(painter.max_texture_side().unwrap_or(2048));
        let render_state = painter.render_state().unwrap();
        log::info!(
            "wgpu device: {:?}, swapchain format: {:?}",
            render_state.device,
            render_state.target_format
        );
        Self {
            window,
            painter,
            egui_ctx: egui::Context::default(),
            egui_winit,
            shapes: vec![],
            textures_delta: egui::TexturesDelta::default(),
        }
    }
}

impl<'a> graphics::Backend for Backend<'a> {
    fn window(&self) -> &winit::window::Window {
        &self.window
    }

    fn paint(&mut self) {
        self.painter.paint_and_update_textures(
            self.egui_ctx.pixels_per_point(),
            egui::Rgba::BLACK,
            &self.egui_ctx.tessellate(std::mem::take(&mut self.shapes)),
            &std::mem::take(&mut self.textures_delta),
        );
    }

    fn egui_ctx(&self) -> &egui::Context {
        &self.egui_ctx
    }

    fn run<'b>(
        &mut self,
        mut run_ui: Box<dyn FnMut(&winit::window::Window, &egui::Context) + 'b>,
    ) -> std::time::Duration {
        let egui::FullOutput {
            platform_output,
            repaint_after,
            textures_delta,
            shapes,
        } = self.egui_ctx.run(self.egui_winit.take_egui_input(&self.window), |ui| {
            run_ui(&self.window, ui)
        });

        self.egui_winit
            .handle_platform_output(&self.window, &self.egui_ctx, platform_output);

        self.shapes = shapes;
        self.textures_delta = textures_delta;
        repaint_after
    }

    fn on_window_event(&mut self, event: &winit::event::WindowEvent) -> bool {
        match event {
            winit::event::WindowEvent::Resized(physical_size) => {
                if physical_size.width > 0 && physical_size.height > 0 {
                    self.painter
                        .on_window_resized(physical_size.width, physical_size.height);
                }
            }
            winit::event::WindowEvent::ScaleFactorChanged {
                scale_factor,
                new_inner_size,
                ..
            } => {
                self.egui_winit.set_pixels_per_point(*scale_factor as f32);
                self.painter
                    .on_window_resized(new_inner_size.width, new_inner_size.height);
            }
            _ => {}
        }
        self.egui_winit.on_event(&self.egui_ctx, event)
    }
}
