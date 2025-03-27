use crate::graphics;
use std::sync::Arc;

const VIEWPORT: egui::ViewportId = egui::ViewportId::ROOT;

pub struct Backend {
    window: Arc<winit::window::Window>,
    painter: egui_wgpu::winit::Painter,
    egui_winit: egui_winit::State,
    shapes: Vec<egui::epaint::ClippedShape>,
    textures_delta: egui::TexturesDelta,
}

impl Backend {
    pub fn new(
        window_attributes: winit::window::WindowAttributes,
        event_loop: &winit::event_loop::ActiveEventLoop,
    ) -> Result<Self, anyhow::Error> {
        let window = Arc::new(event_loop.create_window(window_attributes)?);

        let egui_ctx = egui::Context::default();

        let painter = egui_wgpu::winit::Painter::new(
            egui_ctx.clone(),
            egui_wgpu::WgpuConfiguration {
                wgpu_setup: egui_wgpu::WgpuSetup::CreateNew(egui_wgpu::WgpuSetupCreateNew {
                    instance_descriptor: wgpu::InstanceDescriptor {
                        backends: wgpu::Backends::PRIMARY | wgpu::Backends::GL,
                        flags: wgpu::InstanceFlags::empty(),
                        backend_options: wgpu::BackendOptions::default(),
                    },
                    power_preference: wgpu::PowerPreference::LowPower,
                    native_adapter_selector: None,
                    device_descriptor: Arc::new(|_| wgpu::DeviceDescriptor {
                        label: None,
                        required_features: wgpu::Features::default(),
                        required_limits: wgpu::Limits {
                            max_texture_dimension_2d: 4096,
                            ..wgpu::Limits::downlevel_defaults()
                        },
                        memory_hints: wgpu::MemoryHints::MemoryUsage,
                    }),
                    trace_path: None,
                }),
                present_mode: wgpu::PresentMode::Fifo,
                ..Default::default()
            },
            1,
            None,
            false,
            false,
        );

        let mut painter = pollster::block_on(painter);

        pollster::block_on(painter.set_window(VIEWPORT, Some(window.clone())))?;

        let mut egui_winit = egui_winit::State::new(egui_ctx, VIEWPORT, event_loop, None, None, None);
        egui_winit.set_max_texture_side(painter.max_texture_side().unwrap_or(2048));
        let render_state = painter.render_state().unwrap();
        log::info!(
            "wgpu adapter: {:?}, swapchain format: {:?}",
            render_state.adapter.get_info(),
            render_state.target_format
        );

        Ok(Self {
            window,
            painter,
            egui_winit,
            shapes: vec![],
            textures_delta: egui::TexturesDelta::default(),
        })
    }
}

impl graphics::Backend for Backend {
    fn recreate_window(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        window_attributes: winit::window::WindowAttributes,
    ) {
        self.window = Arc::new(event_loop.create_window(window_attributes).unwrap());
        pollster::block_on(self.painter.set_window(VIEWPORT, Some(self.window.clone()))).unwrap();
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
            Vec::new(),
        );
    }

    fn egui_ctx(&self) -> &egui::Context {
        self.egui_winit.egui_ctx()
    }

    fn run(&mut self, run_ui: &mut dyn FnMut(&egui::Context)) -> std::time::Duration {
        let egui::FullOutput {
            platform_output,
            textures_delta,
            shapes,
            pixels_per_point: _,
            viewport_output,
        } = {
            let egui_input = self.egui_winit.take_egui_input(&self.window);
            self.egui_ctx().run(egui_input, run_ui)
        };

        self.egui_winit.handle_platform_output(&self.window, platform_output);

        self.shapes = shapes;
        self.textures_delta = textures_delta;

        viewport_output[&VIEWPORT].repaint_delay
    }

    fn on_window_event(&mut self, event: &winit::event::WindowEvent) -> egui_winit::EventResponse {
        if let winit::event::WindowEvent::Resized(physical_size) = event {
            if let (Ok(width), Ok(height)) = (physical_size.width.try_into(), physical_size.height.try_into()) {
                self.painter.on_window_resized(VIEWPORT, width, height);
            }
        }

        self.egui_winit.on_window_event(&self.window, event)
    }
}
