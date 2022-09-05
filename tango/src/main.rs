#![windows_subsystem = "windows"]

#[macro_use]
extern crate lazy_static;

mod audio;
mod battle;
mod config;
mod game;
mod gui;
mod i18n;
mod input;
mod lockstep;
mod net;
mod patch;
mod randomcode;
mod replay;
mod replayer;
mod rom;
mod save;
mod scanner;
mod session;
mod shadow;
mod stats;
mod video;

use fluent_templates::Loader;
use glow::HasContext;

const TANGO_CHILD_ENV_VAR: &str = "TANGO_CHILD";

enum UserEvent {
    RequestRepaint,
}

fn main() -> Result<(), anyhow::Error> {
    std::env::set_var("RUST_BACKTRACE", "1");

    env_logger::Builder::from_default_env()
        .filter(Some("tango"), log::LevelFilter::Info)
        .filter(Some("datachannel"), log::LevelFilter::Info)
        .filter(Some("mgba"), log::LevelFilter::Info)
        .init();

    log::info!(
        "welcome to tango v{}-{}!",
        env!("CARGO_PKG_VERSION"),
        git_version::git_version!()
    );

    let config = config::Config::load_or_create()?;
    config.ensure_dirs()?;

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

    let log_file = match std::fs::File::create(&log_path) {
        Ok(f) => f,
        Err(e) => {
            rfd::MessageDialog::new()
                .set_title(
                    &i18n::LOCALES
                        .lookup(&config.language, "window-title")
                        .unwrap(),
                )
                .set_description(
                    &i18n::LOCALES
                        .lookup_with_args(
                            &config.language,
                            "crash-no-log",
                            &std::collections::HashMap::from([(
                                "error",
                                format!("{:?}", e).into(),
                            )]),
                        )
                        .unwrap(),
                )
                .set_level(rfd::MessageLevel::Error)
                .show();
            return Err(e.into());
        }
    };

    let status = std::process::Command::new(std::env::current_exe()?)
        .args(
            std::env::args_os()
                .skip(1)
                .collect::<Vec<std::ffi::OsString>>(),
        )
        .env(TANGO_CHILD_ENV_VAR, "1")
        .stderr(log_file)
        .spawn()?
        .wait()?;

    if !status.success() {
        rfd::MessageDialog::new()
            .set_title("Tango")
            .set_description(
                &i18n::LOCALES
                    .lookup_with_args(
                        &config.language,
                        "crash",
                        &std::collections::HashMap::from([(
                            "path",
                            format!("{}", log_path.display()).into(),
                        )]),
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

fn child_main(config: config::Config) -> Result<(), anyhow::Error> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    mgba::log::init();

    let handle = rt.handle().clone();

    let sdl = sdl2::init().unwrap();
    let audio = sdl.audio().unwrap();
    let game_controller = sdl.game_controller().unwrap();

    let event_loop = glutin::event_loop::EventLoopBuilder::with_user_event().build();
    let mut sdl_event_loop = sdl.event_pump().unwrap();

    let icon = image::load_from_memory(include_bytes!("icon.png"))?;
    let icon_width = icon.width();
    let icon_height = icon.height();

    let wb = winit::window::WindowBuilder::new()
        .with_title("Tango")
        .with_window_icon(Some(winit::window::Icon::from_rgba(
            icon.into_bytes(),
            icon_width,
            icon_height,
        )?))
        .with_inner_size(glutin::dpi::LogicalSize::new(
            mgba::gba::SCREEN_WIDTH * 3,
            mgba::gba::SCREEN_HEIGHT * 3,
        ))
        .with_min_inner_size(glutin::dpi::LogicalSize::new(
            mgba::gba::SCREEN_WIDTH,
            mgba::gba::SCREEN_HEIGHT,
        ))
        .with_fullscreen(if config.full_screen {
            Some(winit::window::Fullscreen::Borderless(None))
        } else {
            None
        });

    let gl_window = glutin::ContextBuilder::new()
        .with_depth_buffer(0)
        .with_stencil_buffer(0)
        .with_vsync(true)
        .build_windowed(wb, &event_loop)
        .unwrap();
    let gl_window = unsafe { gl_window.make_current().unwrap() };

    let gl = std::sync::Arc::new(unsafe {
        glow::Context::from_loader_function(|s| gl_window.get_proc_address(s))
    });
    unsafe {
        gl.clear_color(0.0, 0.0, 0.0, 1.0);
        gl.clear(glow::COLOR_BUFFER_BIT);
    }
    gl_window.swap_buffers().unwrap();

    log::info!("GL version: {}", unsafe {
        gl.get_parameter_string(glow::VERSION)
    });

    let mut egui_glow = egui_glow::EguiGlow::new(&event_loop, gl.clone());

    let audio_binder = audio::LateBinder::new(48000);

    #[cfg(feature = "cpal")]
    let (_audio_device, _stream) = {
        log::info!("using cpal audio");

        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

        let audio_device = cpal::default_host()
            .default_output_device()
            .ok_or_else(|| anyhow::format_err!("could not open audio device"))?;
        log::info!(
            "supported audio output configs: {:?}",
            audio_device.supported_output_configs()?.collect::<Vec<_>>()
        );
        let audio_supported_config = audio::cpal::get_supported_config(&audio_device)?;
        log::info!("selected audio config: {:?}", audio_supported_config);

        let audio_binder = audio::LateBinder::new(audio_supported_config.sample_rate().0);
        let stream =
            audio::cpal::open_stream(&audio_device, &audio_supported_config, audio_binder.clone())?;
        stream.play()?;

        (audio_device, stream)
    };

    #[cfg(not(feature = "cpal"))]
    let _audio_device = {
        log::info!("using sdl2 audio");

        let audio_device = audio::sdl2::open_stream(
            &audio,
            &sdl2::audio::AudioSpecDesired {
                freq: Some(48000),
                channels: Some(audio::NUM_CHANNELS as u8),
                samples: Some(512),
            },
            audio_binder.clone(),
        )
        .unwrap();
        log::info!("audio spec: {:?}", audio_device.spec());
        audio_device.resume();
        audio_device
    };

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

    let mut state = gui::State::new(
        &egui_glow.egui_ctx,
        std::sync::Arc::new(parking_lot::RwLock::new(config)),
        audio_binder.clone(),
        fps_counter.clone(),
        emu_tps_counter.clone(),
    );

    rayon::spawn({
        let config = state.config.clone();
        let patches_scanner = state.patches_scanner.clone();
        move || loop {
            let (repo_url, patches_path) = {
                let config = config.read();
                (
                    if !config.patch_repo.is_empty() {
                        config.patch_repo.clone()
                    } else {
                        config::DEFAULT_PATCH_REPO.to_owned()
                    },
                    config.patches_path().to_path_buf(),
                )
            };
            patches_scanner.rescan(move || match patch::update(&repo_url, &patches_path) {
                Ok(patches) => Some(patches),
                Err(e) => {
                    log::error!("failed to update patches: {:?}", e);
                    return None;
                }
            });
            std::thread::sleep(std::time::Duration::from_secs(30 * 60));
        }
    });

    egui_glow.egui_ctx.set_request_repaint_callback({
        let el_proxy = parking_lot::Mutex::new(event_loop.create_proxy());
        move || {
            let _ = el_proxy.lock().send_event(UserEvent::RequestRepaint);
        }
    });

    event_loop.run(move |event, _, control_flow| {
        let mut config = state.config.read().clone();
        let old_config = config.clone();

        let mut redraw = || {
            let repaint_after = egui_glow.run(gl_window.window(), |ctx| {
                ctx.set_pixels_per_point(
                    gl_window.window().scale_factor() as f32 * config.ui_scale_percent as f32
                        / 100.0,
                );
                gui::show(
                    ctx,
                    &mut config,
                    handle.clone(),
                    gl_window.window(),
                    &input_state,
                    &mut state,
                )
            });

            if repaint_after.is_zero() {
                gl_window.window().request_redraw();
                control_flow.set_poll();
            } else if let Some(repaint_after_instant) =
                std::time::Instant::now().checked_add(repaint_after)
            {
                control_flow.set_wait_until(repaint_after_instant);
            } else {
                control_flow.set_wait();
            }

            unsafe {
                gl.clear_color(0.0, 0.0, 0.0, 1.0);
                gl.clear(glow::COLOR_BUFFER_BIT);
            }
            egui_glow.paint(gl_window.window());
            gl_window.swap_buffers().unwrap();
            fps_counter.lock().mark();
        };

        match event {
            winit::event::Event::WindowEvent {
                event: window_event,
                ..
            } => {
                match window_event {
                    winit::event::WindowEvent::MouseInput { .. }
                    | winit::event::WindowEvent::CursorMoved { .. } => {
                        state.last_mouse_motion_time = Some(std::time::Instant::now());
                        if state.steal_input.is_none() {
                            egui_glow.on_event(&window_event);
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
                                    &mut config.input_mapping,
                                );
                            } else {
                                if !egui_glow.on_event(&window_event) {
                                    input_state.handle_key_down(virutal_keycode);
                                }
                            }
                        }
                        winit::event::ElementState::Released => {
                            if !egui_glow.on_event(&window_event) {
                                input_state.handle_key_up(virutal_keycode);
                            }
                        }
                    },
                    window_event => {
                        egui_glow.on_event(&window_event);
                        match window_event {
                            winit::event::WindowEvent::Focused(false) => {
                                input_state.clear_keys();
                            }
                            winit::event::WindowEvent::Resized(size) => {
                                gl_window.resize(size);
                            }
                            winit::event::WindowEvent::Occluded(false) => {
                                config.full_screen = gl_window.window().fullscreen().is_some();
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
                gl_window.window().request_redraw();
            }
            winit::event::Event::NewEvents(cause) => {
                input_state.digest();
                if let winit::event::StartCause::ResumeTimeReached { .. } = cause {
                    gl_window.window().request_redraw();
                }
            }
            winit::event::Event::UserEvent(UserEvent::RequestRepaint) => {
                gl_window.window().request_redraw();
            }
            winit::event::Event::MainEventsCleared => {
                // We use SDL for controller events and that's it.
                for sdl_event in sdl_event_loop.poll_iter() {
                    (|| match sdl_event {
                        sdl2::event::Event::ControllerDeviceAdded { which, .. } => {
                            if game_controller.is_game_controller(which) {
                                let controller = game_controller.open(which).unwrap();
                                log::info!("controller added: {}", controller.name());
                                controllers.insert(which, controller);
                                input_state.handle_controller_connected(
                                    which,
                                    sdl2::sys::SDL_GameControllerAxis::SDL_CONTROLLER_AXIS_MAX
                                        as usize,
                                );
                            }
                        }
                        sdl2::event::Event::ControllerDeviceRemoved { which, .. } => {
                            if let Some(controller) = controllers.remove(&which) {
                                log::info!("controller removed: {}", controller.name());
                                input_state.handle_controller_disconnected(which);
                            }
                        }
                        sdl2::event::Event::ControllerAxisMotion {
                            axis, value, which, ..
                        } => {
                            if value > input::AXIS_THRESHOLD || value < -input::AXIS_THRESHOLD {
                                if let Some(steal_input) = state.steal_input.take() {
                                    steal_input.run_callback(
                                        input::PhysicalInput::Axis {
                                            axis,
                                            direction: if value > input::AXIS_THRESHOLD {
                                                input::AxisDirection::Positive
                                            } else {
                                                input::AxisDirection::Negative
                                            },
                                        },
                                        &mut config.input_mapping,
                                    );
                                } else {
                                    input_state.handle_controller_axis_motion(
                                        which,
                                        axis as usize,
                                        value,
                                    );
                                }
                            }
                            input_state.handle_controller_axis_motion(which, axis as usize, value);
                        }
                        sdl2::event::Event::ControllerButtonDown { button, which, .. } => {
                            if let Some(steal_input) = state.steal_input.take() {
                                steal_input.run_callback(
                                    input::PhysicalInput::Button(button),
                                    &mut config.input_mapping,
                                );
                            } else {
                                input_state.handle_controller_button_down(which, button);
                            }
                        }
                        sdl2::event::Event::ControllerButtonUp { button, which, .. } => {
                            input_state.handle_controller_button_up(which, button);
                        }
                        _ => {}
                    })();
                }
            }

            winit::event::Event::RedrawEventsCleared if cfg!(windows) => redraw(),
            winit::event::Event::RedrawRequested(_) if !cfg!(windows) => redraw(),

            _ => {}
        }

        if config != old_config {
            *state.config.write() = config.clone();
            let r = config.save();
            log::info!("config save: {:?}", r);
        }
    });
}
