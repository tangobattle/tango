use glow::HasContext;
use glutin::context::NotCurrentGlContext;
use glutin::context::PossiblyCurrentGlContext;
use glutin::display::GetGlDisplay;
use glutin::display::GlDisplay;
use glutin::prelude::GlSurface;
use wgpu::rwh::HasWindowHandle;

use crate::graphics;

pub struct Backend {
    window: winit::window::Window,
    gl: std::sync::Arc<glow::Context>,
    gl_config: glutin::config::Config,
    gl_context: glutin::context::PossiblyCurrentContext,
    gl_surface: glutin::surface::Surface<glutin::surface::WindowSurface>,
    egui_glow: egui_glow::EguiGlow,
}

impl Backend {
    pub fn new(
        window_attributes: winit::window::WindowAttributes,
        event_loop: &winit::event_loop::ActiveEventLoop,
    ) -> Result<Self, anyhow::Error> {
        let (window, gl_config) = glutin_winit::DisplayBuilder::new()
            .with_window_attributes(Some(window_attributes))
            .with_preference(glutin_winit::ApiPreference::FallbackEgl)
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

        let Some(window) = window else {
            anyhow::bail!("failed to create window through glutin_winit");
        };

        log::info!("found gl_config: {:?}", &gl_config);

        let raw_window_handle = window.window_handle().ok().map(|handle| handle.as_raw());

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

        let gl_context = not_current_gl_context.treat_as_possibly_current();
        let gl_surface = Self::create_surface(&window, &gl_config, &gl_context)?;
        let gl_display = gl_context.display();

        let gl = std::sync::Arc::new(unsafe {
            glow::Context::from_loader_function(|s| gl_display.get_proc_address(&std::ffi::CString::new(s).unwrap()))
        });

        let egui_glow = egui_glow::EguiGlow::new(event_loop, gl.clone(), None, None, false);

        log::info!(
            "GL version: {}, extensions: {:?}",
            unsafe { gl.get_parameter_string(glow::VERSION) },
            gl.supported_extensions()
        );

        Ok(Self {
            window,
            gl,
            gl_config,
            gl_context,
            gl_surface,
            egui_glow,
        })
    }

    fn create_surface(
        window: &winit::window::Window,
        gl_config: &glutin::config::Config,
        gl_context: &glutin::context::PossiblyCurrentContext,
    ) -> Result<glutin::surface::Surface<glutin::surface::WindowSurface>, anyhow::Error> {
        let (width, height): (u32, u32) = window.inner_size().into();

        let surface_attributes = glutin::surface::SurfaceAttributesBuilder::<glutin::surface::WindowSurface>::new()
            .build(
                window.window_handle()?.as_raw(),
                std::num::NonZeroU32::new(width.max(1)).unwrap(),
                std::num::NonZeroU32::new(height.max(1)).unwrap(),
            );

        let gl_display = gl_config.display();
        let gl_surface = unsafe { gl_display.create_window_surface(gl_config, &surface_attributes) }?;

        gl_context.make_current(&gl_surface)?;

        if let Err(e) = gl_surface.set_swap_interval(
            gl_context,
            glutin::surface::SwapInterval::Wait(std::num::NonZeroU32::new(1).unwrap()),
        ) {
            log::warn!(
                "failed to set swap interval (may cause tearing or high GPU usage!): {}",
                e
            );
        }

        Ok(gl_surface)
    }
}

impl graphics::Backend for Backend {
    fn recreate_window(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        window_attributes: winit::window::WindowAttributes,
    ) {
        self.window = glutin_winit::finalize_window(event_loop, window_attributes, &self.gl_config).unwrap();
        self.gl_surface = Self::create_surface(&self.window, &self.gl_config, &self.gl_context).unwrap()
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

    fn run(&mut self, run_ui: &mut dyn FnMut(&egui::Context)) -> std::time::Duration {
        self.egui_glow.run(&self.window, run_ui);

        // egui_glow eats the ViewportOutput it seems
        std::time::Duration::ZERO
    }

    fn on_window_event(&mut self, event: &winit::event::WindowEvent) -> egui_winit::EventResponse {
        if let winit::event::WindowEvent::Resized(physical_size) = event {
            if physical_size.width > 0 && physical_size.height > 0 {
                self.gl_surface.resize(
                    &self.gl_context,
                    physical_size.width.try_into().unwrap(),
                    physical_size.height.try_into().unwrap(),
                );
            }
        }

        self.egui_glow.on_window_event(&self.window, event)
    }

    fn exiting(&mut self) {
        self.egui_glow.destroy();
    }
}
