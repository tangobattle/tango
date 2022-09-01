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

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use fluent_templates::Loader;
use glow::HasContext;

const TANGO_CHILD_ENV_VAR: &str = "TANGO_CHILD";

enum UserEvent {
    RequestRepaint,
}

fn main() -> Result<(), anyhow::Error> {
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

    let project_dirs = config::get_project_dirs().unwrap();

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

    let logs_dir = project_dirs.data_dir().join("logs");
    std::fs::create_dir_all(&logs_dir)?;
    let log_path = logs_dir.join(log_filename);
    log::info!("logging to: {}", log_path.display());

    let log_file = std::fs::File::create(&log_path)?;

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
        native_dialog::MessageDialog::new()
            .set_title("Tango")
            .set_text(
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
            .set_type(native_dialog::MessageType::Error)
            .show_alert()
            .unwrap();
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
    let game_controller = sdl.game_controller().unwrap();

    let event_loop = glutin::event_loop::EventLoopBuilder::with_user_event().build();
    let mut sdl_event_loop = sdl.event_pump().unwrap();

    let icon = image::load_from_memory(include_bytes!("icon.png"))?;
    let icon_width = icon.width();
    let icon_height = icon.height();

    let wb = glutin::window::WindowBuilder::new()
        .with_title("Tango")
        .with_window_icon(Some(glutin::window::Icon::from_rgba(
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
            Some(glutin::window::Fullscreen::Borderless(None))
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
    let mut gui = gui::Gui::new(&egui_glow.egui_ctx);

    let audio_device = cpal::default_host()
        .default_output_device()
        .ok_or_else(|| anyhow::format_err!("could not open audio device"))?;
    log::info!(
        "supported audio output configs: {:?}",
        audio_device.supported_output_configs()?.collect::<Vec<_>>()
    );
    let audio_supported_config = audio::get_supported_config(&audio_device)?;
    log::info!("selected audio config: {:?}", audio_supported_config);

    let audio_binder = audio::LateBinder::new(audio_supported_config.clone());
    let stream = audio::open_stream(&audio_device, &audio_supported_config, audio_binder.clone())?;
    stream.play()?;

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
        std::sync::Arc::new(parking_lot::RwLock::new(config)),
        audio_binder.clone(),
        fps_counter.clone(),
        emu_tps_counter.clone(),
    );

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
                gui.show(
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
            glutin::event::Event::WindowEvent {
                event: window_event,
                ..
            } => {
                match window_event {
                    glutin::event::WindowEvent::MouseInput { .. }
                    | glutin::event::WindowEvent::CursorMoved { .. } => {
                        if state.steal_input.is_none() {
                            egui_glow.on_event(&window_event);
                        }
                    }
                    glutin::event::WindowEvent::KeyboardInput {
                        input:
                            glutin::event::KeyboardInput {
                                virtual_keycode: Some(virutal_keycode),
                                state: element_state,
                                ..
                            },
                        ..
                    } => match element_state {
                        glutin::event::ElementState::Pressed => {
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
                        glutin::event::ElementState::Released => {
                            if !egui_glow.on_event(&window_event) {
                                input_state.handle_key_up(virutal_keycode);
                            }
                        }
                    },
                    window_event => {
                        egui_glow.on_event(&window_event);
                        match window_event {
                            glutin::event::WindowEvent::Focused(false) => {
                                input_state.clear_keys();
                            }
                            glutin::event::WindowEvent::Resized(size) => {
                                gl_window.resize(size);
                            }
                            glutin::event::WindowEvent::Occluded(false) => {
                                config.full_screen = gl_window.window().fullscreen().is_some();
                            }
                            glutin::event::WindowEvent::CloseRequested => {
                                control_flow.set_exit();
                            }
                            _ => {}
                        }
                    }
                };
                gl_window.window().request_redraw();
            }
            glutin::event::Event::NewEvents(cause) => {
                input_state.digest();
                if let glutin::event::StartCause::ResumeTimeReached { .. } = cause {
                    gl_window.window().request_redraw();
                }
            }
            glutin::event::Event::UserEvent(UserEvent::RequestRepaint) => {
                gl_window.window().request_redraw();
            }
            glutin::event::Event::MainEventsCleared => {
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

            glutin::event::Event::RedrawEventsCleared if cfg!(windows) => redraw(),
            glutin::event::Event::RedrawRequested(_) if !cfg!(windows) => redraw(),

            _ => {}
        }

        if config != old_config {
            *state.config.write() = config.clone();
            let r = config.save();
            log::info!("config save: {:?}", r);
        }
    });
}
