use crate::{audio, battle, gui, input, ipc, session, stats, video};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use glow::HasContext;
use parking_lot::Mutex;
use std::sync::Arc;

pub const EXPECTED_FPS: f32 = 60.0;

pub struct State {
    pub session: Option<session::Session>,
    pub video_filter: Box<dyn video::Filter>,
}

pub fn run(
    rt: tokio::runtime::Runtime,
    ipc_sender: Arc<Mutex<ipc::Sender>>,
    window_title: String,
    input_mapping: input::Mapping,
    rom_path: std::path::PathBuf,
    save_path: std::path::PathBuf,
    window_scale: u32,
    video_filter: Box<dyn video::Filter>,
    match_init: Option<battle::MatchInit>,
) -> Result<(), anyhow::Error> {
    let handle = rt.handle().clone();

    let title_prefix = format!("Tango: {}", window_title);

    let sdl = sdl2::init().unwrap();
    let game_controller = sdl.game_controller().unwrap();

    // let _window = video.window("", 0, 0).hidden().build().unwrap();

    let event_loop = glutin::event_loop::EventLoop::new();
    let mut sdl_event_loop = sdl.event_pump().unwrap();

    let wb = glutin::window::WindowBuilder::new()
        .with_title(window_title.clone())
        .with_inner_size(glutin::dpi::LogicalSize::new(
            mgba::gba::SCREEN_WIDTH * window_scale,
            mgba::gba::SCREEN_HEIGHT * window_scale,
        ))
        .with_min_inner_size(glutin::dpi::LogicalSize::new(
            mgba::gba::SCREEN_WIDTH,
            mgba::gba::SCREEN_HEIGHT,
        ));

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

    let mut gui = gui::Gui::new();
    let mut egui_glow = egui_glow::EguiGlow::new(&event_loop, gl.clone());

    let audio_device = cpal::default_host()
        .default_output_device()
        .ok_or_else(|| anyhow::format_err!("could not open audio device"))?;
    log::info!(
        "supported audio output configs: {:?}",
        audio_device.supported_output_configs()?.collect::<Vec<_>>()
    );
    let audio_supported_config = audio::get_supported_config(&audio_device)?;
    log::info!("selected audio config: {:?}", audio_supported_config);

    let audio_binder = audio::LateBinder::new();
    let stream = audio::open_stream(&audio_device, &audio_supported_config, audio_binder.clone())?;
    stream.play()?;

    let fps_counter = Arc::new(Mutex::new(stats::Counter::new(30)));
    let emu_tps_counter = Arc::new(Mutex::new(stats::Counter::new(10)));

    let mut input_state = input_helper::State::new();

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

    let mut state = State {
        session: None,
        video_filter,
    };

    state.session = Some(session::Session::new(
        rt.handle().clone(),
        ipc_sender.clone(),
        audio_binder.clone(),
        audio_supported_config.sample_rate(),
        rom_path,
        save_path,
        emu_tps_counter.clone(),
        match_init,
    )?);

    rt.block_on(async {
        ipc_sender
            .lock()
            .send(ipc::protos::FromCoreMessage {
                which: Some(ipc::protos::from_core_message::Which::StateEv(
                    ipc::protos::from_core_message::StateEvent {
                        state: ipc::protos::from_core_message::state_event::State::Running.into(),
                    },
                )),
            })
            .await?;
        anyhow::Result::<()>::Ok(())
    })?;

    event_loop.run(move |event, _, control_flow| {
        control_flow.set_poll();

        match event {
            glutin::event::Event::WindowEvent {
                event: window_event,
                ..
            } => match window_event {
                glutin::event::WindowEvent::Resized(size) => {
                    gl_window.resize(size);
                }
                glutin::event::WindowEvent::CloseRequested => {
                    control_flow.set_exit();
                }
                glutin::event::WindowEvent::KeyboardInput {
                    input:
                        glutin::event::KeyboardInput {
                            scancode, state, ..
                        },
                    ..
                } => match state {
                    glutin::event::ElementState::Pressed => {
                        input_state.handle_key_down(scancode as usize);
                    }
                    glutin::event::ElementState::Released => {
                        input_state.handle_key_up(scancode as usize);
                    }
                },
                _ => {}
            },
            glutin::event::Event::MainEventsCleared => {
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
                            input_state.handle_controller_axis_motion(which, axis as usize, value);
                        }
                        sdl2::event::Event::ControllerButtonDown { button, which, .. } => {
                            input_state.handle_controller_button_down(which, button as usize);
                        }
                        sdl2::event::Event::ControllerButtonUp { button, which, .. } => {
                            input_state.handle_controller_button_up(which, button as usize);
                        }
                        _ => {}
                    }
                }
                if let Some(session) = &state.session {
                    session.set_joyflags(input_mapping.to_mgba_keys(&input_state))
                }

                gl_window.window().request_redraw();
            }

            glutin::event::Event::RedrawRequested(_) => {
                unsafe {
                    gl.clear_color(0.0, 0.0, 0.0, 1.0);
                    gl.clear(glow::COLOR_BUFFER_BIT);
                }

                let is_session_active = state
                    .session
                    .as_ref()
                    .map(|s| {
                        s.match_()
                            .as_ref()
                            .map(|match_| handle.block_on(async { match_.lock().await.is_some() }))
                            .unwrap_or(true)
                    })
                    .unwrap_or(false);
                if !is_session_active {
                    state.session = None;
                }

                if state.session.is_none() {
                    *control_flow = glutin::event_loop::ControlFlow::Exit;
                    return;
                }

                // HACK: No better place to put this for now.
                if let Some(session) = &state.session {
                    // If we're in single-player mode, allow speedup.
                    if session.match_().is_none() {
                        session.set_fps(
                            if input_mapping
                                .speed_up
                                .iter()
                                .any(|c| c.is_active(&input_state))
                            {
                                EXPECTED_FPS * 3.0
                            } else {
                                EXPECTED_FPS
                            },
                        );
                    }

                    // If we've crashed, log the error and panic.
                    if let Some(thread_handle) = session.has_crashed() {
                        // HACK: No better way to lock the core.
                        let audio_guard = thread_handle.lock_audio();
                        panic!(
                            "mgba thread crashed!\nlr = {:08x}, pc = {:08x}",
                            audio_guard.core().gba().cpu().gpr(14),
                            audio_guard.core().gba().cpu().thumb_pc()
                        );
                    }

                    // Update title to show P1/P2 state.
                    let mut title = title_prefix.to_string();
                    if let Some(match_) = session.match_().as_ref() {
                        rt.block_on(async {
                            if let Some(match_) = &*match_.lock().await {
                                let round_state = match_.lock_round_state().await;
                                if let Some(round) = round_state.round.as_ref() {
                                    title =
                                        format!("{} [P{}]", title, round.local_player_index() + 1);
                                }
                            }
                        });
                    }

                    gl_window.window().set_title(&title);
                }

                egui_glow.run(gl_window.window(), |ctx| gui.draw(ctx, &mut state));
                egui_glow.paint(gl_window.window());

                gl_window.swap_buffers().unwrap();
                fps_counter.lock().mark();
            }

            _ => {}
        }
    });
}
