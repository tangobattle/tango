use crate::{audio, battle, facade, gui, hooks, ipc, negotiation, tps};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;
use std::sync::Arc;

pub const EXPECTED_FPS: u32 = 60;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Keymapping {
    pub up: winit::event::VirtualKeyCode,
    pub down: winit::event::VirtualKeyCode,
    pub left: winit::event::VirtualKeyCode,
    pub right: winit::event::VirtualKeyCode,
    pub a: winit::event::VirtualKeyCode,
    pub b: winit::event::VirtualKeyCode,
    pub l: winit::event::VirtualKeyCode,
    pub r: winit::event::VirtualKeyCode,
    pub select: winit::event::VirtualKeyCode,
    pub start: winit::event::VirtualKeyCode,
}

pub struct Game {
    rt: tokio::runtime::Runtime,
    gui: gui::Gui,
    ipc_client: ipc::Client,
    fps_counter: Arc<Mutex<tps::Counter>>,
    event_loop: Option<winit::event_loop::EventLoop<UserEvent>>,
    _audio_device: cpal::Device,
    _primary_mux_handle: audio::mux_stream::MuxHandle,
    window: winit::window::Window,
    pixels: pixels::Pixels,
    vbuf: Arc<Mutex<Vec<u8>>>,
    _stream: cpal::Stream,
    joyflags: Arc<std::sync::atomic::AtomicU32>,
    keymapping: Keymapping,
    _thread: mgba::thread::Thread,
}

enum UserEvent {
    Gilrs(gilrs::Event),
}

impl Game {
    pub fn new(
        mut ipc_client: ipc::Client,
        window_title: String,
        keymapping: Keymapping,
        rom_path: std::path::PathBuf,
        save_path: std::path::PathBuf,
        match_settings: Option<battle::Settings>,
    ) -> Result<Game, anyhow::Error> {
        let audio_device = cpal::default_host()
            .default_output_device()
            .ok_or_else(|| anyhow::format_err!("could not open audio device"))?;
        log::info!(
            "supported audio output configs: {:?}",
            audio_device.supported_output_configs()?.collect::<Vec<_>>()
        );

        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;

        let handle = rt.handle().clone();

        let negotiation = if let Some(match_settings) = match_settings.as_ref() {
            Some(handle.block_on(async {
                negotiation::negotiate(
                    &mut ipc_client,
                    &match_settings.session_id,
                    &match_settings.matchmaking_connect_addr,
                    &match_settings.ice_servers,
                )
                .await
            })?)
        } else {
            None
        };

        let event_loop = Some(winit::event_loop::EventLoop::with_user_event());

        let vbuf = Arc::new(Mutex::new(vec![
            0u8;
            (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT * 4)
                as usize
        ]));

        let window = {
            let size = winit::dpi::LogicalSize::new(
                mgba::gba::SCREEN_WIDTH * 3,
                mgba::gba::SCREEN_HEIGHT * 3,
            );
            winit::window::WindowBuilder::new()
                .with_title(window_title.clone())
                .with_inner_size(size)
                .with_min_inner_size(size)
                .build(event_loop.as_ref().expect("event loop"))?
        };

        let fps_counter = Arc::new(Mutex::new(tps::Counter::new(30)));
        let emu_tps_counter = Arc::new(Mutex::new(tps::Counter::new(10)));

        let (pixels, gui) = {
            let window_size = window.inner_size();
            let surface_texture =
                pixels::SurfaceTexture::new(window_size.width, window_size.height, &window);
            let pixels = pixels::PixelsBuilder::new(
                mgba::gba::SCREEN_WIDTH,
                mgba::gba::SCREEN_HEIGHT,
                surface_texture,
            )
            .build()?;
            let gui = gui::Gui::new(
                window_size.width,
                window_size.height,
                window.scale_factor() as f32,
                &pixels,
            );
            (pixels, gui)
        };

        let mut core = mgba::core::Core::new_gba("tango")?;
        core.enable_video_buffer();

        let rom_vf = mgba::vfile::VFile::open(&rom_path, mgba::vfile::flags::O_RDONLY)?;
        core.as_mut().load_rom(rom_vf)?;

        let save_vf = mgba::vfile::VFile::open(
            &save_path,
            mgba::vfile::flags::O_CREAT | mgba::vfile::flags::O_RDWR,
        )?;
        core.as_mut().load_save(save_vf)?;

        let hooks = hooks::HOOKS.get(&core.as_ref().game_title()).unwrap();

        let joyflags = Arc::new(std::sync::atomic::AtomicU32::new(0));

        let audio_supported_config = audio::get_supported_config(&audio_device)?;
        log::info!("selected audio config: {:?}", audio_supported_config);

        let cancellation_token = tokio_util::sync::CancellationToken::new();

        let match_ = std::sync::Arc::new(tokio::sync::Mutex::new(None));
        if let Some(_) = match_settings {
            core.set_traps(hooks.primary_traps(
                handle.clone(),
                facade::Facade::new(match_.clone(), joyflags.clone(), cancellation_token.clone()),
            ));
        }

        let thread = mgba::thread::Thread::new(core);

        let audio_mux = audio::mux_stream::MuxStream::new();
        let primary_mux_handle = audio_mux.open_stream(audio::mgba_stream::MGBAStream::new(
            thread.handle(),
            audio_supported_config.sample_rate(),
        ));

        if let Some(match_settings) = match_settings {
            let negotiation = negotiation.unwrap();

            let _ = std::fs::create_dir_all(&match_settings.replays_path);

            let match_ = match_.clone();
            handle.block_on(async {
                let is_offerer = negotiation.peer_conn.local_description().unwrap().sdp_type
                    == datachannel_wrapper::SdpType::Offer;
                *match_.lock().await = Some(std::sync::Arc::new(battle::Match::new(
                    audio_supported_config.clone(),
                    rom_path.clone(),
                    hooks,
                    audio_mux.clone(),
                    negotiation.peer_conn,
                    negotiation.dc,
                    negotiation.rng,
                    is_offerer,
                    thread.handle(),
                    match_settings,
                )));
            });

            handle.spawn(async move {
                {
                    let match_ = match_.lock().await.clone().unwrap();
                    tokio::select! {
                        Err(e) = match_.run() => {
                            log::info!("match thread ending: {:?}", e);
                        }
                        _ = cancellation_token.cancelled() => {
                        }
                    }
                }
                *match_.lock().await = None;
            });
        }

        thread.start()?;
        thread
            .handle()
            .lock_audio()
            .core_mut()
            .gba_mut()
            .sync_mut()
            .as_mut()
            .unwrap()
            .set_fps_target(EXPECTED_FPS as f32);

        {
            let joyflags = joyflags.clone();
            let vbuf = vbuf.clone();
            let emu_tps_counter = emu_tps_counter.clone();
            thread.set_frame_callback(move |mut core, video_buffer| {
                let mut vbuf = vbuf.lock();
                vbuf.copy_from_slice(video_buffer);
                for i in (0..vbuf.len()).step_by(4) {
                    vbuf[i + 3] = 0xff;
                }
                core.set_keys(joyflags.load(std::sync::atomic::Ordering::Relaxed));
                let mut emu_tps_counter = emu_tps_counter.lock();
                emu_tps_counter.mark();
            });
        }

        let stream = audio::open_stream(&audio_device, &audio_supported_config, audio_mux.clone())?;
        stream.play()?;

        let gui_state = gui.state();
        {
            let match_ = Arc::downgrade(&match_);
            let fps_counter = fps_counter.clone();
            let emu_tps_counter = emu_tps_counter.clone();
            let handle = handle;
            gui_state.set_debug_stats_getter(Some(Box::new(move || {
                handle.block_on(async {
                    let emu_tps_counter = emu_tps_counter.lock();
                    let fps_counter = fps_counter.lock();
                    Some(gui::DebugStats {
                        fps: 1.0 / fps_counter.mean_duration().as_secs_f32(),
                        emu_tps: 1.0 / emu_tps_counter.mean_duration().as_secs_f32(),
                        match_: {
                            match match_.upgrade() {
                                Some(match_) => match &*match_.lock().await {
                                    Some(match_) => Some(gui::MatchDebugStats {
                                        round: {
                                            let round_state = match_.lock_round_state().await;
                                            match &round_state.round {
                                                Some(round) => Some(gui::RoundDebugStats {
                                                    local_player_index: round.local_player_index(),
                                                    local_qlen: round.local_queue_length(),
                                                    remote_qlen: round.remote_queue_length(),
                                                    local_delay: round.local_delay(),
                                                    remote_delay: round.remote_delay(),
                                                    tps_adjustment: round.tps_adjustment(),
                                                }),
                                                None => None,
                                            }
                                        },
                                    }),
                                    None => None,
                                },
                                None => None,
                            }
                        },
                    })
                })
            })));
        }

        Ok(Game {
            rt,
            gui,
            ipc_client,
            _audio_device: audio_device,
            _primary_mux_handle: primary_mux_handle,
            keymapping,
            fps_counter,
            event_loop,
            window,
            pixels,
            vbuf,
            _stream: stream,
            joyflags,
            _thread: thread,
        })
    }

    pub fn run(mut self) -> anyhow::Result<()> {
        self.rt.block_on(async {
            self.ipc_client
                .send_notification(ipc::Notification::State(ipc::State::Running))
                .await?;
            anyhow::Result::<()>::Ok(())
        })?;

        let mut gilrs = gilrs::Gilrs::new().unwrap();
        for (_id, gamepad) in gilrs.gamepads() {
            log::info!(
                "found gamepad: {} is {:?}",
                gamepad.name(),
                gamepad.power_info()
            );
        }

        let el_proxy = self.event_loop.as_ref().expect("event loop").create_proxy();
        std::thread::spawn(move || {
            while let Some(event) = gilrs.next_event() {
                if let Err(_) = el_proxy.send_event(UserEvent::Gilrs(event)) {
                    break;
                }
            }
        });

        let mut console_key_pressed = false;

        self.event_loop
            .take()
            .expect("event loop")
            .run(move |event, _, control_flow| {
                *control_flow = winit::event_loop::ControlFlow::Poll;

                match event {
                    winit::event::Event::WindowEvent {
                        event: ref window_event,
                        ..
                    } => {
                        match window_event {
                            winit::event::WindowEvent::KeyboardInput { input, .. } => {
                                let mut keymask = 0u32;
                                if input.virtual_keycode == Some(self.keymapping.left) {
                                    keymask |= mgba::input::keys::LEFT;
                                }
                                if input.virtual_keycode == Some(self.keymapping.right) {
                                    keymask |= mgba::input::keys::RIGHT;
                                }
                                if input.virtual_keycode == Some(self.keymapping.up) {
                                    keymask |= mgba::input::keys::UP;
                                }
                                if input.virtual_keycode == Some(self.keymapping.down) {
                                    keymask |= mgba::input::keys::DOWN;
                                }
                                if input.virtual_keycode == Some(self.keymapping.a) {
                                    keymask |= mgba::input::keys::A;
                                }
                                if input.virtual_keycode == Some(self.keymapping.b) {
                                    keymask |= mgba::input::keys::B;
                                }
                                if input.virtual_keycode == Some(self.keymapping.l) {
                                    keymask |= mgba::input::keys::L;
                                }
                                if input.virtual_keycode == Some(self.keymapping.r) {
                                    keymask |= mgba::input::keys::R;
                                }
                                if input.virtual_keycode == Some(self.keymapping.start) {
                                    keymask |= mgba::input::keys::START;
                                }
                                if input.virtual_keycode == Some(self.keymapping.select) {
                                    keymask |= mgba::input::keys::SELECT;
                                }

                                match input.state {
                                    winit::event::ElementState::Pressed => {
                                        self.joyflags.fetch_or(
                                            keymask,
                                            std::sync::atomic::Ordering::Relaxed,
                                        );
                                    }
                                    winit::event::ElementState::Released => {
                                        self.joyflags.fetch_and(
                                            !keymask,
                                            std::sync::atomic::Ordering::Relaxed,
                                        );
                                    }
                                }

                                if input.virtual_keycode
                                    == Some(winit::event::VirtualKeyCode::Grave)
                                {
                                    match input.state {
                                        winit::event::ElementState::Pressed => {
                                            if console_key_pressed {
                                                return;
                                            }
                                            console_key_pressed = true;
                                            self.gui.state().toggle_debug();
                                        }
                                        winit::event::ElementState::Released => {
                                            console_key_pressed = false;
                                        }
                                    }
                                }
                            }
                            winit::event::WindowEvent::CloseRequested => {
                                *control_flow = winit::event_loop::ControlFlow::Exit;
                            }
                            winit::event::WindowEvent::Resized(size) => {
                                self.pixels.resize_surface(size.width, size.height);
                                self.gui.resize(size.width, size.height);
                            }
                            _ => {}
                        };

                        self.gui.handle_event(window_event);
                    }
                    winit::event::Event::MainEventsCleared => {
                        let vbuf = self.vbuf.lock().clone();
                        self.pixels.get_frame().copy_from_slice(&vbuf);

                        self.gui.prepare(&self.window);
                        self.pixels
                            .render_with(|encoder, render_target, context| {
                                context.scaling_renderer.render(encoder, render_target);
                                self.gui.render(encoder, render_target, context)?;
                                Ok(())
                            })
                            .expect("render pixels");
                        self.fps_counter.lock().mark();
                    }
                    winit::event::Event::UserEvent(UserEvent::Gilrs(gilrs_ev)) => {
                        log::info!("{:?}", gilrs_ev);
                    }
                    _ => {}
                }
            });
    }
}
