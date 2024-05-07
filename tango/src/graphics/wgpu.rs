use crate::graphics;
use std::sync::Arc;

const VIEWPORT: egui::ViewportId = egui::ViewportId::ROOT;

pub struct Backend {
    window: Arc<winit::window::Window>,
    painter: egui_wgpu::winit::Painter,
    egui_winit: egui_winit::State,
    shapes: Vec<egui::epaint::ClippedShape>,
    textures_delta: egui::TexturesDelta,
    ui_scale: f32,
}

impl Backend {
    pub fn new<T>(
        wb: winit::window::WindowBuilder,
        event_loop: &winit::event_loop::EventLoopWindowTarget<T>,
    ) -> Result<Self, anyhow::Error> {
        let window = Arc::new(wb.build(event_loop)?);

        let mut painter = egui_wgpu::winit::Painter::new(
            egui_wgpu::WgpuConfiguration {
                device_descriptor: Arc::new(|_| wgpu::DeviceDescriptor {
                    label: None,
                    required_features: wgpu::Features::default(),
                    required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
                }),
                supported_backends: wgpu::Backends::PRIMARY | wgpu::Backends::GL,
                present_mode: wgpu::PresentMode::Fifo,
                power_preference: wgpu::PowerPreference::LowPower,
                ..Default::default()
            },
            1,
            None,
            false,
        );

        pollster::block_on(painter.set_window(VIEWPORT, Some(window.clone())))?;

        let egui_ctx = egui::Context::default();

        let mut egui_winit = egui_winit::State::new(egui_ctx, VIEWPORT, event_loop, None, None);
        egui_winit.egui_ctx().set_pixels_per_point(window.scale_factor() as f32);
        egui_winit.set_max_texture_side(painter.max_texture_side().unwrap_or(2048));
        let render_state = painter.render_state().unwrap();
        log::info!(
            "wgpu device: {:?}, swapchain format: {:?}",
            render_state.device,
            render_state.target_format
        );
        Ok(Self {
            window,
            painter,
            egui_winit,
            shapes: vec![],
            textures_delta: egui::TexturesDelta::default(),
            ui_scale: 1.0,
        })
    }
}

impl graphics::Backend for Backend {
    fn set_ui_scale(&mut self, scale: f32) {
        self.ui_scale = scale;
        self.egui_ctx()
            .set_pixels_per_point(self.window.scale_factor() as f32 * self.ui_scale);
    }

    fn window(&self) -> &winit::window::Window {
        &self.window
    }

    fn paint(&mut self) {
        let pixels_per_point = self.egui_ctx().pixels_per_point();
        let shapes = std::mem::take(&mut self.shapes);
        let clipped_primitives = self.egui_ctx().tessellate(shapes, pixels_per_point);

        self.painter.paint_and_update_textures(
            VIEWPORT,
            pixels_per_point,
            [0.0, 0.0, 0.0, 1.0],
            &clipped_primitives,
            &std::mem::take(&mut self.textures_delta),
            false,
        );
    }

    fn egui_ctx(&self) -> &egui::Context {
        self.egui_winit.egui_ctx()
    }

    fn run(&mut self, run_ui: &mut dyn FnMut(&winit::window::Window, &egui::Context)) -> std::time::Duration {
        let egui::FullOutput {
            platform_output,
            textures_delta,
            shapes,
            pixels_per_point: _,
            viewport_output,
        } = {
            let egui_input = self.egui_winit.take_egui_input(&self.window);
            self.egui_ctx().run(egui_input, |ui| run_ui(&self.window, ui))
        };

        self.egui_winit.handle_platform_output(&self.window, platform_output);

        self.shapes = shapes;
        self.textures_delta = textures_delta;

        viewport_output[&VIEWPORT].repaint_delay
    }

    fn on_window_event(&mut self, event: &winit::event::WindowEvent) -> egui_winit::EventResponse {
        match event {
            winit::event::WindowEvent::Resized(physical_size) => {
                if let (Ok(width), Ok(height)) = (physical_size.width.try_into(), physical_size.height.try_into()) {
                    self.painter.on_window_resized(VIEWPORT, width, height);
                }
            }
            winit::event::WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.egui_ctx()
                    .set_pixels_per_point(*scale_factor as f32 * self.ui_scale);

                let inner_size = self.window.inner_size();

                if let (Ok(width), Ok(height)) = (inner_size.width.try_into(), inner_size.height.try_into()) {
                    self.painter.on_window_resized(VIEWPORT, width, height);
                }
            }
            _ => {}
        }
        self.egui_winit.on_window_event(&self.window, event)
    }
}
