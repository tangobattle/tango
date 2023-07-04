use glow::HasContext;
use glutin::context::NotCurrentGlContextSurfaceAccessor;
use glutin::display::GetGlDisplay;
use glutin::display::GlDisplay;
use glutin::prelude::GlSurface;
use raw_window_handle::HasRawWindowHandle;

use crate::graphics;

pub struct Backend {
    window: winit::window::Window,
    gl: std::sync::Arc<glow::Context>,
    gl_context: glutin::context::PossiblyCurrentContext,
    gl_surface: glutin::surface::Surface<glutin::surface::WindowSurface>,
    egui_glow: egui_glow::EguiGlow,
    ui_scale: f32,
}

impl Backend {
    pub fn new<T>(
        wb: winit::window::WindowBuilder,
        event_loop: &winit::event_loop::EventLoopWindowTarget<T>,
    ) -> Result<Self, anyhow::Error> {
        let (window, gl_config) = glutin_winit::DisplayBuilder::new()
            .with_preference(glutin_winit::ApiPrefence::FallbackEgl)
            .with_window_builder(Some(wb.clone()))
            .build(
                event_loop,
                glutin::config::ConfigTemplateBuilder::new()
                    .prefer_hardware_accelerated(None)
                    .with_depth_size(0)
                    .with_stencil_size(0)
                    .with_transparency(false),
                |mut config_iterator| {
                    config_iterator
                        .next()
                        .expect("failed to find a matching configuration for creating glutin config")
                },
            )
            .expect("failed to create gl_config");

        let gl_display = gl_config.display();
        log::info!("found gl_config: {:?}", &gl_config);

        let raw_window_handle = window.as_ref().map(|w| w.raw_window_handle());

        let not_current_gl_context = if let Some(ctx) = [
            glutin::context::ContextAttributesBuilder::new(),
            glutin::context::ContextAttributesBuilder::new().with_context_api(glutin::context::ContextApi::Gles(None)),
            glutin::context::ContextAttributesBuilder::new().with_context_api(glutin::context::ContextApi::OpenGl(
                Some(glutin::context::Version::new(2, 1)),
            )),
        ]
        .into_iter()
        .flat_map(|cab| {
            let ca = cab.build(raw_window_handle);
            unsafe { gl_config.display().create_context(&gl_config, &ca) }
                .map_err(|e| {
                    log::warn!("failed to create gl context with attributes {:?}: {}", ca, e);
                    e
                })
                .ok()
        })
        .next()
        {
            ctx
        } else {
            anyhow::bail!("all attempts at creating a gl context failed");
        };

        let window = match window {
            Some(window) => window,
            None => glutin_winit::finalize_window(event_loop, wb.clone(), &gl_config)?,
        };

        let (width, height): (u32, u32) = window.inner_size().into();
        let surface_attributes = glutin::surface::SurfaceAttributesBuilder::<glutin::surface::WindowSurface>::new()
            .build(
                window.raw_window_handle(),
                std::num::NonZeroU32::new(std::cmp::max(width, 1)).unwrap(),
                std::num::NonZeroU32::new(std::cmp::max(height, 1)).unwrap(),
            );

        let gl_surface = unsafe { gl_display.create_window_surface(&gl_config, &surface_attributes) }?;

        let gl_context = not_current_gl_context.make_current(&gl_surface)?;

        if let Err(e) = gl_surface.set_swap_interval(
            &gl_context,
            glutin::surface::SwapInterval::Wait(std::num::NonZeroU32::new(1).unwrap()),
        ) {
            log::warn!(
                "failed to set swap interval (may cause tearing or high GPU usage!): {}",
                e
            );
        }

        let gl = std::sync::Arc::new(unsafe {
            glow::Context::from_loader_function(|s| gl_display.get_proc_address(&std::ffi::CString::new(s).unwrap()))
        });

        let mut egui_glow = egui_glow::EguiGlow::new(&event_loop, gl.clone(), None);
        egui_glow.egui_winit.set_pixels_per_point(window.scale_factor() as f32);

        log::info!(
            "GL version: {}, extensions: {:?}",
            unsafe { gl.get_parameter_string(glow::VERSION) },
            gl.supported_extensions()
        );

        Ok(Self {
            window,
            gl,
            gl_context,
            gl_surface,
            egui_glow,
            ui_scale: 1.0,
        })
    }
}

impl graphics::Backend for Backend {
    fn set_ui_scale(&mut self, scale: f32) {
        self.ui_scale = scale;
        self.egui_glow
            .egui_winit
            .set_pixels_per_point(self.window.scale_factor() as f32 * self.ui_scale);
    }

    fn window(&self) -> &winit::window::Window {
        &self.window
    }

    fn paint(&mut self) {
        unsafe {
            self.gl.clear_color(0.0, 0.0, 0.0, 1.0);
            self.gl.clear(glow::COLOR_BUFFER_BIT);
        }
        self.egui_glow.paint(&self.window);
        self.gl_surface.swap_buffers(&self.gl_context).unwrap();
    }

    fn egui_ctx(&self) -> &egui::Context {
        &self.egui_glow.egui_ctx
    }

    fn run(&mut self, run_ui: &mut dyn FnMut(&winit::window::Window, &egui::Context)) -> std::time::Duration {
        self.egui_glow.run(&self.window, |ui| run_ui(&self.window, ui))
    }

    fn on_window_event(&mut self, event: &winit::event::WindowEvent) -> egui_winit::EventResponse {
        match event {
            winit::event::WindowEvent::Resized(physical_size) => {
                if physical_size.width > 0 && physical_size.height > 0 {
                    self.gl_surface.resize(
                        &self.gl_context,
                        physical_size.width.try_into().unwrap(),
                        physical_size.height.try_into().unwrap(),
                    );
                }
            }
            winit::event::WindowEvent::ScaleFactorChanged {
                new_inner_size,
                scale_factor,
            } => {
                self.egui_glow
                    .egui_winit
                    .set_pixels_per_point(*scale_factor as f32 * self.ui_scale);
                self.gl_surface.resize(
                    &self.gl_context,
                    new_inner_size.width.try_into().unwrap(),
                    new_inner_size.height.try_into().unwrap(),
                );
            }
            _ => {}
        }
        self.egui_glow.on_event(event)
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        self.egui_glow.destroy();
    }
}
