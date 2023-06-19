#![windows_subsystem = "windows"]

use std::io::Write;

use clap::Parser;

#[macro_use]
extern crate lazy_static;

mod audio;
mod config;
mod discord;
mod game;
mod graphics;
mod gui;
mod i18n;
mod input;
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

enum UserEvent {
    RequestRepaint,
}

fn main() -> Result<(), anyhow::Error> {
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
        return child_main(config);
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

fn child_main(mut config: config::Config) -> Result<(), anyhow::Error> {
    let args = Args::parse();

    let init_link_code = match args.command {
        Some(Command::Join { link_code }) => Some(link_code),
        _ => None,
    };

    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    let _enter_guard = rt.enter();

    let mut show_update_info = false;
    if config.last_version != version::current() {
        config.last_version = version::current();
        show_update_info = true;
    }

    config.save()?;
    let config = std::sync::Arc::new(parking_lot::RwLock::new(config));

    let updater_path = config::get_updater_path().unwrap();
    let _ = std::fs::create_dir_all(&updater_path);
    let mut updater = updater::Updater::new(&updater_path, config.clone());
    updater.set_enabled(config.read().enable_updater);

    let sdl = sdl2::init().unwrap();
    let game_controller = sdl.game_controller().unwrap();

    let event_loop = winit::event_loop::EventLoopBuilder::with_user_event().build();
    let mut sdl_event_loop = sdl.event_pump().unwrap();

    let icon = image::load_from_memory(include_bytes!("icon.png"))?;
    let icon_width = icon.width();
    let icon_height = icon.height();

    let window_builder = winit::window::WindowBuilder::new()
        .with_title(&i18n::LOCALES.lookup(&config.read().language, "window-title").unwrap())
        .with_window_icon(Some(winit::window::Icon::from_rgba(
            icon.into_bytes(),
            icon_width,
            icon_height,
        )?))
        .with_inner_size(config.read().window_size.clone())
        .with_min_inner_size(winit::dpi::LogicalSize::new(
            mgba::gba::SCREEN_WIDTH,
            mgba::gba::SCREEN_HEIGHT,
        ))
        .with_fullscreen(if config.read().full_screen {
            Some(winit::window::Fullscreen::Borderless(None))
        } else {
            None
        });

    let mut gfx_backend: Box<dyn graphics::Backend> = match config.read().graphics_backend {
        #[cfg(feature = "glutin")]
        config::GraphicsBackend::Glutin => Box::new(graphics::glutin::Backend::new(window_builder, &event_loop)?),
        #[cfg(feature = "wgpu")]
        config::GraphicsBackend::Wgpu => Box::new(graphics::wgpu::Backend::new(window_builder, &event_loop)?),
    };
    gfx_backend.set_ui_scale(config.read().ui_scale_percent as f32 / 100.0);
    gfx_backend.run(Box::new(|_, _| {}));
    gfx_backend.paint();

    let egui_ctx = gfx_backend.egui_ctx();
    egui_ctx.set_request_repaint_callback({
        let el_proxy = parking_lot::Mutex::new(event_loop.create_proxy());
        move || {
            let _ = el_proxy.lock().send_event(UserEvent::RequestRepaint);
        }
    });
    updater.set_ui_callback({
        let egui_ctx = egui_ctx.clone();
        Some(Box::new(move || {
            egui_ctx.request_repaint();
        }))
    });

    let mut audio_binder = audio::LateBinder::new();
    let audio_backend: Box<dyn audio::Backend> = match config.read().audio_backend {
        #[cfg(feature = "cpal")]
        config::AudioBackend::Cpal => Box::new(audio::cpal::Backend::new(audio_binder.clone())?),
        #[cfg(feature = "sdl2-audio")]
        config::AudioBackend::Sdl2 => Box::new(audio::sdl2::Backend::new(&sdl, audio_binder.clone())?),
    };
    audio_binder.set_sample_rate(audio_backend.sample_rate());

    let fps_counter = std::sync::Arc::new(parking_lot::Mutex::new(stats::Counter::new(30)));
    let emu_tps_counter = std::sync::Arc::new(parking_lot::Mutex::new(stats::Counter::new(10)));

    let mut input_state = input::State::new();

    let mut controllers: std::collections::HashMap<u32, sdl2::controller::GameController> =
        std::collections::HashMap::new();
    // Preemptively enumerate controllers.
    for which in 0..game_controller.num_joysticks().unwrap() {
        if !game_controller.is_game_controller(which) {
            continue;
        }
        let controller = game_controller.open(which).unwrap();
        log::info!("controller added: {}", controller.name());
        controllers.insert(which, controller);
    }

    let discord_client = discord::Client::new();

    let roms_scanner = scanner::Scanner::new();
    let saves_scanner = scanner::Scanner::new();
    let patches_scanner = scanner::Scanner::new();
    {
        let roms_path = config.read().roms_path();
        let saves_path = config.read().saves_path();
        let patches_path = config.read().patches_path();
        roms_scanner.rescan(move || Some(game::scan_roms(&roms_path)));
        saves_scanner.rescan(move || Some(save::scan_saves(&saves_path)));
        patches_scanner.rescan(move || Some(patch::scan(&patches_path).unwrap_or_default()));
    }

    let mut state = gui::State::new(
        egui_ctx,
        show_update_info,
        config.clone(),
        discord_client,
        audio_binder.clone(),
        fps_counter.clone(),
        emu_tps_counter.clone(),
        roms_scanner.clone(),
        saves_scanner.clone(),
        patches_scanner.clone(),
        init_link_code,
    )?;

    let mut patch_autoupdater = patch::Autoupdater::new(config.clone(), patches_scanner.clone());
    patch_autoupdater.set_enabled(config.read().enable_patch_autoupdate);

    let mut last_config_dirty_time = None;
    event_loop.run(move |event, _, control_flow| {
        let mut next_config = config.read().clone();
        let old_config = next_config.clone();

        let mut redraw = || {
            let repaint_after = gfx_backend.run(Box::new(|window, ctx| {
                gui::show(ctx, &mut next_config, window, &input_state, &mut state, &updater)
            }));

            if repaint_after.is_zero() {
                gfx_backend.window().request_redraw();
                control_flow.set_poll();
            } else if let Some(repaint_after_instant) = std::time::Instant::now().checked_add(repaint_after) {
                control_flow.set_wait_until(repaint_after_instant);
            } else {
                control_flow.set_wait();
            }

            gfx_backend.paint();
            fps_counter.lock().mark();
        };

        match event {
            winit::event::Event::WindowEvent {
                event: window_event, ..
            } => {
                match window_event {
                    winit::event::WindowEvent::MouseInput { .. } | winit::event::WindowEvent::CursorMoved { .. } => {
                        state.last_mouse_motion_time = Some(std::time::Instant::now());
                        if state.steal_input.is_none() {
                            let _ = gfx_backend.on_window_event(&window_event);
                        }
                    }
                    winit::event::WindowEvent::KeyboardInput {
                        input:
                            winit::event::KeyboardInput {
                                virtual_keycode: Some(virutal_keycode),
                                state: element_state,
                                ..
                            },
                        ..
                    } => match element_state {
                        winit::event::ElementState::Pressed => {
                            if let Some(steal_input) = state.steal_input.take() {
                                steal_input.run_callback(
                                    input::PhysicalInput::Key(virutal_keycode),
                                    &mut next_config.input_mapping,
                                );
                            } else {
                                if !gfx_backend.on_window_event(&window_event).consumed {
                                    input_state.handle_key_down(virutal_keycode);
                                } else {
                                    input_state.clear_keys();
                                }
                            }
                        }
                        winit::event::ElementState::Released => {
                            if !gfx_backend.on_window_event(&window_event).consumed {
                                input_state.handle_key_up(virutal_keycode);
                            } else {
                                input_state.clear_keys();
                            }
                        }
                    },
                    window_event => {
                        let _ = gfx_backend.on_window_event(&window_event);
                        match window_event {
                            winit::event::WindowEvent::Focused(false) => {
                                input_state.clear_keys();
                            }
                            winit::event::WindowEvent::Occluded(false) => {
                                next_config.full_screen = gfx_backend.window().fullscreen().is_some();
                            }
                            winit::event::WindowEvent::CursorEntered { .. } => {
                                state.last_mouse_motion_time = Some(std::time::Instant::now());
                            }
                            winit::event::WindowEvent::CursorLeft { .. } => {
                                state.last_mouse_motion_time = None;
                            }
                            winit::event::WindowEvent::CloseRequested => {
                                control_flow.set_exit();
                            }
                            _ => {}
                        }
                    }
                };
                gfx_backend.window().request_redraw();
            }
            winit::event::Event::NewEvents(cause) => {
                input_state.digest();
                if let winit::event::StartCause::ResumeTimeReached { .. } = cause {
                    gfx_backend.window().request_redraw();
                }
            }
            winit::event::Event::UserEvent(UserEvent::RequestRepaint) => {
                gfx_backend.window().request_redraw();
            }
            winit::event::Event::MainEventsCleared => {
                // We use SDL for controller events and that's it.
                for sdl_event in sdl_event_loop.poll_iter() {
                    match sdl_event {
                        sdl2::event::Event::ControllerDeviceAdded { which, .. } => {
                            if game_controller.is_game_controller(which) {
                                let controller = game_controller.open(which).unwrap();
                                log::info!("controller added: {}", controller.name());
                                controllers.insert(which, controller);
                                input_state.handle_controller_connected(
                                    which,
                                    sdl2::sys::SDL_GameControllerAxis::SDL_CONTROLLER_AXIS_MAX as usize,
                                );
                            }
                        }
                        sdl2::event::Event::ControllerDeviceRemoved { which, .. } => {
                            if let Some(controller) = controllers.remove(&which) {
                                log::info!("controller removed: {}", controller.name());
                                input_state.handle_controller_disconnected(which);
                            }
                        }
                        sdl2::event::Event::ControllerAxisMotion { axis, value, which, .. } => {
                            if let Some(steal_input) = (value > input::AXIS_THRESHOLD || value < -input::AXIS_THRESHOLD)
                                .then(|| state.steal_input.take())
                                .flatten()
                            {
                                steal_input.run_callback(
                                    input::PhysicalInput::Axis {
                                        axis,
                                        direction: if value > input::AXIS_THRESHOLD {
                                            input::AxisDirection::Positive
                                        } else {
                                            input::AxisDirection::Negative
                                        },
                                    },
                                    &mut next_config.input_mapping,
                                );
                            } else {
                                input_state.handle_controller_axis_motion(which, axis as usize, value);
                            }
                            gfx_backend.window().request_redraw();
                        }
                        sdl2::event::Event::ControllerButtonDown { button, which, .. } => {
                            if let Some(steal_input) = state.steal_input.take() {
                                steal_input
                                    .run_callback(input::PhysicalInput::Button(button), &mut next_config.input_mapping);
                            } else {
                                input_state.handle_controller_button_down(which, button);
                            }
                            gfx_backend.window().request_redraw();
                        }
                        sdl2::event::Event::ControllerButtonUp { button, which, .. } => {
                            input_state.handle_controller_button_up(which, button);
                            gfx_backend.window().request_redraw();
                        }
                        _ => {}
                    }
                }
            }

            winit::event::Event::RedrawEventsCleared if cfg!(windows) => redraw(),
            winit::event::Event::RedrawRequested(_) if !cfg!(windows) => redraw(),

            _ => {}
        }

        if let Some(session) = state.session.lock().as_mut() {
            session.set_joyflags(next_config.input_mapping.to_mgba_keys(&input_state));
            session.set_master_volume(next_config.volume);
        }

        next_config.window_size = gfx_backend
            .window()
            .inner_size()
            .to_logical(gfx_backend.window().scale_factor());

        if next_config != old_config {
            last_config_dirty_time = Some(std::time::Instant::now());
            *config.write() = next_config.clone();
        }

        if last_config_dirty_time
            .map(|t| (std::time::Instant::now() - t) > std::time::Duration::from_secs(1))
            .unwrap_or(false)
        {
            let r = next_config.save();
            log::info!("config flushed: {:?}", r);
            last_config_dirty_time = None;
        }

        gfx_backend.set_ui_scale(next_config.ui_scale_percent as f32 / 100.0);
        patch_autoupdater.set_enabled(next_config.enable_patch_autoupdate);
        updater.set_enabled(next_config.enable_updater);
    });
}
