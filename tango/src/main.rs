#![windows_subsystem = "windows"]

use std::io::Write;

use clap::Parser;

#[macro_use]
extern crate lazy_static;

mod audio;
mod config;
mod controller;
mod discord;
mod fonts;
mod game;
mod graphics;
mod gui;
mod i18n;
mod input;
mod keyboard;
mod net;
mod patch;
mod randomcode;
mod rom;
mod save;
mod scanner;
mod session;
mod stats;
mod sync;
mod updater;
mod version;
mod video;

use fluent_templates::Loader;
use keyboard::Key;

const TANGO_CHILD_ENV_VAR: &str = "TANGO_CHILD";

#[derive(clap::Parser)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Join.
    Join {
        /// Link code to join.
        link_code: String,
    },
}

pub enum WindowRequest {
    Repaint,
    Attention,
    SetTitle(String),
    SetFullscreen(Option<winit::window::Fullscreen>),
    SetWindowSize(winit::dpi::PhysicalSize<u32>),
}

fn main() -> Result<(), anyhow::Error> {
    let start_instant = std::time::Instant::now();
    let args = Args::parse();

    std::env::set_var("RUST_BACKTRACE", "1");

    let config = config::Config::load_or_create()?;
    config.ensure_dirs()?;

    env_logger::Builder::from_default_env()
        .filter(Some("tango"), log::LevelFilter::Info)
        .filter(Some("datachannel"), log::LevelFilter::Info)
        .filter(Some("mgba"), log::LevelFilter::Info)
        .init();

    log::info!("welcome to tango {}!", version::current());

    if std::env::var(TANGO_CHILD_ENV_VAR).unwrap_or_default() == "1" {
        return child_main(config, start_instant, args);
    }

    let log_filename = format!(
        "{}.log",
        time::OffsetDateTime::from(std::time::SystemTime::now())
            .format(time::macros::format_description!(
                "[year padding:zero][month padding:zero repr:numerical][day padding:zero][hour padding:zero][minute padding:zero][second padding:zero]"
            ))
            .expect("format time"),
    );

    let log_path = config.logs_path().join(log_filename);
    log::info!("logging to: {}", log_path.display());

    let mut log_file = match std::fs::File::create(&log_path) {
        Ok(f) => f,
        Err(e) => {
            rfd::MessageDialog::new()
                .set_title(&i18n::LOCALES.lookup(&config.language, "window-title").unwrap())
                .set_description(
                    &i18n::LOCALES
                        .lookup_with_args(
                            &config.language,
                            "crash-no-log",
                            &std::collections::HashMap::from([("error", format!("{:?}", e).into())]),
                        )
                        .unwrap(),
                )
                .set_level(rfd::MessageLevel::Error)
                .show();
            return Err(e.into());
        }
    };

    let status = std::process::Command::new(std::env::current_exe()?)
        .args(std::env::args_os().skip(1).collect::<Vec<std::ffi::OsString>>())
        .env(TANGO_CHILD_ENV_VAR, "1")
        .stderr(log_file.try_clone()?)
        .spawn()?
        .wait()?;

    writeln!(&mut log_file, "exit status: {:?}", status)?;

    if !status.success() {
        rfd::MessageDialog::new()
            .set_title(&i18n::LOCALES.lookup(&config.language, "window-title").unwrap())
            .set_description(
                &i18n::LOCALES
                    .lookup_with_args(
                        &config.language,
                        "crash",
                        &std::collections::HashMap::from([("path", format!("{}", log_path.display()).into())]),
                    )
                    .unwrap(),
            )
            .set_level(rfd::MessageLevel::Error)
            .show();
    }

    if let Some(code) = status.code() {
        std::process::exit(code);
    }

    Ok(())
}

fn child_main(config: config::Config, start_instant: std::time::Instant, args: Args) -> Result<(), anyhow::Error> {
    let init_link_code = match args.command {
        Some(Command::Join { link_code }) => Some(link_code),
        _ => None,
    };

    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    let _enter_guard = rt.enter();

    let event_loop = winit::event_loop::EventLoop::with_user_event().build().unwrap();

    let mut app = TangoWinitApp::new(&event_loop, config, start_instant, init_link_code);
    event_loop.run_app(&mut app)?;

    Ok(())
}

#[cfg(not(target_os = "android"))]
struct SdlSystems {
    sdl: sdl2::Sdl,
    event_loop: sdl2::EventPump,
    controller_subsystem: sdl2::GameControllerSubsystem,
    controllers: std::collections::HashMap<u32, sdl2::controller::GameController>,
}

#[cfg(not(target_os = "android"))]
impl SdlSystems {
    fn new() -> Self {
        let sdl = sdl2::init().unwrap();
        let event_loop = sdl.event_pump().unwrap();
        let controller_subsystem = sdl.game_controller().unwrap();

        // init controller map
        let mut controllers: std::collections::HashMap<u32, sdl2::controller::GameController> =
            std::collections::HashMap::new();

        for which in 0..controller_subsystem.num_joysticks().unwrap() {
            if !controller_subsystem.is_game_controller(which) {
                continue;
            }
            match controller_subsystem.open(which) {
                Ok(controller) => {
                    log::info!("controller added: {}", controller.name());
                    controllers.insert(which, controller);
                }
                Err(e) => {
                    log::info!("failed to add controller: {}", e);
                }
            }
        }

        Self {
            sdl,
            event_loop,
            controller_subsystem,
            controllers,
        }
    }
}

struct TangoWinitApp {
    config: std::sync::Arc<parking_lot::RwLock<config::Config>>,
    start_instant: std::time::Instant,
    init_link_code: Option<String>,
    event_loop_proxy: winit::event_loop::EventLoopProxy<WindowRequest>,
    audio_backend: Option<Box<dyn audio::Backend>>,
    gfx_backend: Option<Box<dyn graphics::Backend>>,
    gui_state: Option<gui::State>,
    scanners: Option<gui::Scanners>,
    updater: updater::Updater,
    patch_autoupdater: patch::Autoupdater,
    last_config_dirty_time: Option<std::time::Instant>,
    input_state: input::State,
    #[cfg(not(target_os = "android"))]
    sdl_systems: SdlSystems,
}

impl TangoWinitApp {
    fn new(
        event_loop: &winit::event_loop::EventLoop<WindowRequest>,
        config: config::Config,
        start_instant: std::time::Instant,
        init_link_code: Option<String>,
    ) -> Self {
        let config = std::sync::Arc::new(parking_lot::RwLock::new(config));
        let event_loop_proxy = event_loop.create_proxy();

        // create client updater
        let updater_path = config::get_updater_path().unwrap();
        let _ = std::fs::create_dir_all(&updater_path);
        let mut updater = updater::Updater::new(&updater_path, config.clone());
        updater.set_ui_callback({
            let el_proxy = event_loop_proxy.clone();
            Some(Box::new(move || {
                let _ = el_proxy.send_event(WindowRequest::Repaint);
            }))
        });
        updater.set_enabled(config.read().enable_updater);

        // init scanners
        let scanners = gui::Scanners::new(&config.read());

        // create patch updater
        let mut patch_autoupdater = patch::Autoupdater::new(config.clone(), scanners.patches.clone());
        patch_autoupdater.set_enabled(config.read().enable_patch_autoupdate);

        Self {
            start_instant,
            config,
            init_link_code,
            event_loop_proxy,
            audio_backend: None,
            gfx_backend: None,
            gui_state: None,
            scanners: Some(scanners),
            updater,
            patch_autoupdater,
            last_config_dirty_time: None,
            input_state: input::State::new(),
            #[cfg(not(target_os = "android"))]
            sdl_systems: SdlSystems::new(),
        }
    }

    fn try_flush_config(&mut self) {
        if self
            .last_config_dirty_time
            .is_some_and(|t| t.elapsed() > std::time::Duration::from_secs(1))
        {
            self.flush_config();
        }
    }

    fn flush_config(&mut self) {
        if self.last_config_dirty_time.is_some() {
            let r = self.config.read().save();
            log::info!("config flushed: {:?}", r);
            self.last_config_dirty_time = None;
        }
    }

    fn request_redraw(&mut self) {
        if let Some(gfx_backend) = &mut self.gfx_backend {
            gfx_backend.window().request_redraw();
        }
    }
}

impl winit::application::ApplicationHandler<WindowRequest> for TangoWinitApp {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        // create icon
        let icon_image = image::load_from_memory(include_bytes!("icon.png")).unwrap();
        let icon_width = icon_image.width();
        let icon_height = icon_image.height();
        let icon = winit::window::Icon::from_rgba(icon_image.into_bytes(), icon_width, icon_height).unwrap();

        // create window attributes
        let window_attributes = winit::window::WindowAttributes::default()
            .with_title(
                i18n::LOCALES
                    .lookup(&self.config.read().language, "window-title")
                    .unwrap(),
            )
            .with_window_icon(Some(icon))
            .with_inner_size(self.config.read().window_size)
            .with_min_inner_size(winit::dpi::PhysicalSize::new(
                mgba::gba::SCREEN_WIDTH,
                mgba::gba::SCREEN_HEIGHT,
            ))
            .with_fullscreen(if self.config.read().full_screen {
                Some(winit::window::Fullscreen::Borderless(None))
            } else {
                None
            });

        // try to update existing gfx backend
        if let Some(gfx_backend) = &mut self.gfx_backend {
            gfx_backend.recreate_window(event_loop, window_attributes);
            return;
        };

        // init graphics backend
        let gfx_backend: Box<dyn graphics::Backend> = match self.config.read().graphics_backend {
            #[cfg(feature = "glutin")]
            config::GraphicsBackend::Glutin => {
                Box::new(graphics::glutin::Backend::new(window_attributes, event_loop).unwrap())
            }
            #[cfg(feature = "wgpu")]
            config::GraphicsBackend::Wgpu => {
                Box::new(graphics::wgpu::Backend::new(window_attributes, event_loop).unwrap())
            }
        };

        let egui_ctx = gfx_backend.egui_ctx();
        egui_extras::install_image_loaders(egui_ctx);
        egui_ctx.set_zoom_factor(self.config.read().ui_scale_percent as f32 / 100.0);
        egui_ctx.set_request_repaint_callback({
            let el_proxy = self.event_loop_proxy.clone();
            move |_| {
                let _ = el_proxy.send_event(WindowRequest::Repaint);
            }
        });

        // init audio
        let mut audio_binder = audio::LateBinder::new();
        let audio_backend: Box<dyn audio::Backend> = match self.config.read().audio_backend {
            #[cfg(feature = "cpal")]
            config::AudioBackend::Cpal => Box::new(audio::cpal::Backend::new(audio_binder.clone()).unwrap()),
            #[cfg(feature = "sdl2-audio")]
            config::AudioBackend::Sdl2 => {
                Box::new(audio::sdl2::Backend::new(&self.sdl_systems.sdl, audio_binder.clone()).unwrap())
            }
        };
        audio_binder.set_sample_rate(audio_backend.sample_rate());

        // keep the audio backend alive for the duration of the application
        self.audio_backend = Some(audio_backend);

        // detect first launch after an update
        let show_update_info = self.config.read().last_version != version::current();
        self.config.write().last_version = version::current();
        self.config.read().save().unwrap();

        // init perf counters
        let fps_counter = std::sync::Arc::new(parking_lot::Mutex::new(stats::Counter::new(30)));
        let emu_tps_counter = std::sync::Arc::new(parking_lot::Mutex::new(stats::Counter::new(10)));

        // init discord client api
        let discord_client = discord::Client::new();

        // init gui_state
        self.gui_state = Some(
            gui::State::new(
                self.event_loop_proxy.clone(),
                egui_ctx,
                show_update_info,
                self.config.clone(),
                discord_client,
                audio_binder,
                fps_counter,
                emu_tps_counter,
                self.scanners.take().unwrap(),
                self.init_link_code.take(),
            )
            .unwrap(),
        );

        self.gfx_backend = Some(gfx_backend);

        log::info!("launched in {:?}s", self.start_instant.elapsed().as_secs_f32());
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        let Some(gfx_backend) = self.gfx_backend.as_mut() else {
            return;
        };

        let gui_state = self.gui_state.as_mut().unwrap();

        let mut next_config = self.config.read().clone();
        let old_config = next_config.clone();

        match event {
            winit::event::WindowEvent::RedrawRequested => {
                let repaint_after = gfx_backend
                    .run(&mut (|ctx| gui::show(ctx, &mut next_config, &self.input_state, gui_state, &self.updater)));

                gfx_backend.paint();
                gui_state.shared.fps_counter.lock().mark();

                if repaint_after.is_zero() {
                    gfx_backend.window().request_redraw();
                    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
                } else if let Some(repaint_after_instant) = std::time::Instant::now().checked_add(repaint_after) {
                    event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(repaint_after_instant));
                } else {
                    event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
                }
            }
            winit::event::WindowEvent::MouseInput { .. } | winit::event::WindowEvent::CursorMoved { .. } => {
                gui_state.last_mouse_motion_time = Some(std::time::Instant::now());
                if gui_state.steal_input.is_none() {
                    let _ = gfx_backend.on_window_event(&event);
                }

                gfx_backend.window().request_redraw();
            }
            winit::event::WindowEvent::KeyboardInput {
                event:
                    winit::event::KeyEvent {
                        physical_key: winit::keyboard::PhysicalKey::Code(winit_key),
                        state: element_state,
                        ..
                    },
                ..
            } => {
                if let Some(key) = Key::resolve(winit_key) {
                    match element_state {
                        winit::event::ElementState::Pressed => {
                            if let Some(steal_input) = gui_state.steal_input.take() {
                                steal_input
                                    .run_callback(input::PhysicalInput::Key(key), &mut next_config.input_mapping);
                            } else if !gfx_backend.on_window_event(&event).consumed {
                                self.input_state.handle_key_down(key);
                            } else {
                                self.input_state.clear_keys();
                            }
                        }
                        winit::event::ElementState::Released => {
                            if !gfx_backend.on_window_event(&event).consumed {
                                self.input_state.handle_key_up(key);
                            } else {
                                self.input_state.clear_keys();
                            }
                        }
                    }

                    gfx_backend.window().request_redraw();
                }
            }
            window_event => {
                if gfx_backend.on_window_event(&window_event).consumed {
                    gfx_backend.window().request_redraw();
                }

                match window_event {
                    winit::event::WindowEvent::Focused(false) => {
                        self.input_state.clear_keys();
                    }
                    winit::event::WindowEvent::Occluded(false) => {
                        next_config.full_screen = gfx_backend.window().fullscreen().is_some();
                    }
                    winit::event::WindowEvent::CursorEntered { .. } => {
                        gui_state.last_mouse_motion_time = Some(std::time::Instant::now());
                    }
                    winit::event::WindowEvent::CursorLeft { .. } => {
                        gui_state.last_mouse_motion_time = None;
                    }
                    winit::event::WindowEvent::CloseRequested => {
                        self.flush_config();
                        event_loop.exit();
                        return;
                    }
                    _ => {}
                }
            }
        }

        if let Some(session) = gui_state.shared.session.lock().as_mut() {
            session.set_joyflags(next_config.input_mapping.to_mgba_keys(&self.input_state));
            session.set_master_volume(next_config.volume);
        }

        next_config.window_size = gfx_backend
            .window()
            .inner_size()
            .to_logical(gfx_backend.window().scale_factor());

        if next_config != old_config {
            let egui_ctx = gfx_backend.egui_ctx();
            egui_ctx.set_zoom_factor(next_config.ui_scale_percent as f32 / 100.0);

            self.patch_autoupdater.set_enabled(next_config.enable_patch_autoupdate);
            self.updater.set_enabled(next_config.enable_updater);

            self.last_config_dirty_time = Some(std::time::Instant::now());
            *self.config.write() = next_config;
        }
    }

    fn new_events(&mut self, _: &winit::event_loop::ActiveEventLoop, cause: winit::event::StartCause) {
        self.input_state.digest();

        if let winit::event::StartCause::ResumeTimeReached { .. } = cause {
            self.request_redraw();
        }
    }

    fn user_event(&mut self, _: &winit::event_loop::ActiveEventLoop, request: WindowRequest) {
        match request {
            WindowRequest::Repaint => self.request_redraw(),
            WindowRequest::Attention => {
                if let Some(gfx_backend) = &self.gfx_backend {
                    let window = gfx_backend.window();
                    window.request_user_attention(Some(winit::window::UserAttentionType::Critical));
                }
            }
            WindowRequest::SetTitle(title) => {
                if let Some(gfx_backend) = &self.gfx_backend {
                    gfx_backend.window().set_title(&title);
                }
            }
            WindowRequest::SetFullscreen(value) => {
                if let Some(gfx_backend) = &self.gfx_backend {
                    gfx_backend.window().set_fullscreen(value)
                }
            }
            WindowRequest::SetWindowSize(size) => {
                if let Some(gfx_backend) = &mut self.gfx_backend {
                    if let Some(size) = gfx_backend.window().request_inner_size(size) {
                        let _ = gfx_backend.on_window_event(&winit::event::WindowEvent::Resized(size));
                    }
                }
            }
        }
    }

    fn about_to_wait(&mut self, _: &winit::event_loop::ActiveEventLoop) {
        // read controller input and update input mapping
        let mut new_mapping = self.config.read().input_mapping.clone();
        let old_mapping = new_mapping.clone();

        let mut request_redraw = false;

        #[cfg(not(target_os = "android"))]
        for sdl_event in self.sdl_systems.event_loop.poll_iter() {
            match sdl_event {
                sdl2::event::Event::ControllerDeviceAdded { which, .. } => {
                    if self.sdl_systems.controller_subsystem.is_game_controller(which) {
                        match self.sdl_systems.controller_subsystem.open(which) {
                            Ok(controller) => {
                                log::info!("controller added: {}", controller.name());

                                // insane: `which` for ControllerDeviceAdded is not the same as the other events
                                // https://github.com/libsdl-org/SDL/issues/7401
                                // this event uses `joystick_index`, the rest work on the joystick's `id`
                                let which = controller.instance_id();

                                self.sdl_systems.controllers.insert(which, controller);
                                self.input_state.handle_controller_connected(
                                    which,
                                    sdl2::sys::SDL_GameControllerAxis::SDL_CONTROLLER_AXIS_MAX as usize,
                                );
                            }
                            Err(e) => {
                                log::info!("failed to add controller: {}", e);
                            }
                        }
                    }
                }
                sdl2::event::Event::ControllerDeviceRemoved { which, .. } => {
                    if let Some(controller) = self.sdl_systems.controllers.remove(&which) {
                        log::info!("controller removed: {}", controller.name());
                        self.input_state.handle_controller_disconnected(which);
                    }
                }
                sdl2::event::Event::ControllerAxisMotion { axis, value, which, .. } => {
                    const AXIS_THRESHOLD_RANGE: std::ops::RangeInclusive<i16> =
                        -input::AXIS_THRESHOLD..=input::AXIS_THRESHOLD;

                    if let Some(steal_input) = (!AXIS_THRESHOLD_RANGE.contains(&value))
                        .then_some(self.gui_state.as_mut())
                        .flatten()
                        .and_then(|gui_state| gui_state.steal_input.take())
                    {
                        steal_input.run_callback(
                            input::PhysicalInput::Axis {
                                axis: axis.into(),
                                direction: if value > input::AXIS_THRESHOLD {
                                    input::AxisDirection::Positive
                                } else {
                                    input::AxisDirection::Negative
                                },
                            },
                            &mut new_mapping,
                        );
                    } else {
                        self.input_state
                            .handle_controller_axis_motion(which, axis as usize, value);
                    }

                    request_redraw = true;
                }
                sdl2::event::Event::ControllerButtonDown { button, which, .. } => {
                    if let Some(steal_input) = self.gui_state.as_mut().and_then(|s| s.steal_input.take()) {
                        steal_input.run_callback(input::PhysicalInput::Button(button.into()), &mut new_mapping);
                    } else {
                        self.input_state.handle_controller_button_down(which, button.into());
                    }

                    request_redraw = true;
                }
                sdl2::event::Event::ControllerButtonUp { button, which, .. } => {
                    self.input_state.handle_controller_button_up(which, button.into());

                    request_redraw = true;
                }
                _ => {}
            }
        }

        if let Some(gui_state) = self.gui_state.as_mut() {
            if let Some(session) = gui_state.shared.session.lock().as_mut() {
                session.set_joyflags(new_mapping.to_mgba_keys(&self.input_state));
            }
        }

        if old_mapping != new_mapping {
            self.last_config_dirty_time = Some(std::time::Instant::now());
            self.config.write().input_mapping = new_mapping;
        }

        self.try_flush_config();

        if request_redraw {
            self.request_redraw();
        }
    }

    fn exiting(&mut self, ev: &winit::event_loop::ActiveEventLoop) {
        use winit::platform::wayland::ActiveEventLoopExtWayland;

        if let Some(backend) = &mut self.gfx_backend {
            backend.exiting();

            // resolve whether we should drop early for this backend and platform
            if backend.should_take_on_exit() || ev.is_wayland() {
                self.gfx_backend.take();
            }
        }
    }
}
